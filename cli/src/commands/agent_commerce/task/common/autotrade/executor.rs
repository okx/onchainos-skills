//! Durable coordination for model-routed subscription trades.
//!
//! The selected Skill/tool owns its transaction command. This module admits a
//! delivery exactly once, persists its bounded outcome, and pushes a
//! job-scoped idempotent UI notice.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{consent, grants, guide, subscription, subscription_config};
use crate::commands::agent_commerce::task::common::{okx_a2a, user_lang};

const OUTCOME_VERSION: u32 = 1;
const NOTICE_REF_VERSION: u32 = 1;
const EXECUTION_LATCH_VERSION: u32 = 2;
const TERMINAL_JOURNAL_VERSION: u32 = 1;
const INITIAL_NOTIFY_TIMEOUT: Duration = Duration::from_secs(5);
const FLUSH_NOTIFY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_NOTIFICATION_ATTEMPTS: u32 = 10;
const STALE_LEASE_SEC: u64 = 30;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum OutcomeStatus {
    Submitted,
    FailedBeforeSubmit,
    UnknownAfterSubmit,
    Skipped,
    FailedBeforeExecution,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FailureCategory {
    AuthenticationRequired,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ExecutionPhase {
    Reserved,
    Prepared,
    Spawned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecutionLatch {
    version: u32,
    job_id: String,
    delivery_id: String,
    phase: ExecutionPhase,
    updated_at: u64,
    /// Retained only to read latches written by older releases. New
    /// Guide-direct claims deliberately do not record a purported amount that
    /// the coordinator cannot bind to the selected tool command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    direct_amount: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    direct_execution_mode: Option<ExecutionMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryState {
    NoExecution,
    PreSubmitInterrupted,
    SubmissionUnknown,
    TerminalOutcome,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[default]
    Auto,
    Manual,
    OneTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionOutcome {
    version: u32,
    job_id: String,
    delivery_id: String,
    venue: String,
    action: String,
    amount: String,
    #[serde(default)]
    execution_mode: ExecutionMode,
    status: OutcomeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failure_category: Option<FailureCategory>,
    notification_pending: bool,
    #[serde(default)]
    notification_attempts: u32,
    #[serde(default)]
    next_notification_attempt_at: u64,
    created_at: u64,
    updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OutcomeNoticeRef {
    version: u32,
    job_id: String,
    delivery_id: String,
    next_attempt_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TerminalJournal {
    version: u32,
    outcome: ExecutionOutcome,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectClaimResult {
    pub allowed: bool,
    pub status: String,
    pub job_id: String,
    pub delivery_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Result of restoring the local execution material without reserving a
/// delivery. A non-ready result is display-only and may be retried by a later
/// signal after the local condition changes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuidePrepareResult {
    pub ready: bool,
    pub status: String,
    pub job_id: String,
    pub delivery_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn safe_delivery_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':' | b'.'))
}

fn outcome_path(job_id: &str, delivery_id: &str) -> Result<PathBuf> {
    if !grants::job_id_is_safe(job_id) || !safe_delivery_id(delivery_id) {
        bail!("invalid job or delivery id");
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("outcomes")
        .join(job_id)
        .join(format!("{delivery_id}.json")))
}

fn latch_path(job_id: &str, delivery_id: &str) -> Result<PathBuf> {
    if !grants::job_id_is_safe(job_id) || !safe_delivery_id(delivery_id) {
        bail!("invalid job or delivery id");
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("execution-latch")
        .join(job_id)
        .join(delivery_id))
}

fn terminal_journal_path(job_id: &str, delivery_id: &str) -> Result<PathBuf> {
    if !grants::job_id_is_safe(job_id) || !safe_delivery_id(delivery_id) {
        bail!("invalid job or delivery id");
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("terminal-journal")
        .join(job_id)
        .join(format!("{delivery_id}.json")))
}

fn notice_index_root() -> Result<PathBuf> {
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("pending-outcome-notifications"))
}

fn notice_ref_path(job_id: &str, delivery_id: &str) -> Result<PathBuf> {
    if !grants::job_id_is_safe(job_id) || !safe_delivery_id(delivery_id) {
        bail!("invalid job or delivery id");
    }
    let name = hex::encode(Sha256::digest(format!("{job_id}\0{delivery_id}")));
    Ok(notice_index_root()?.join(format!("{name}.json")))
}

fn sync_notice_ref(outcome: &ExecutionOutcome) -> Result<()> {
    let path = notice_ref_path(&outcome.job_id, &outcome.delivery_id)?;
    if !outcome.notification_pending {
        let _ = std::fs::remove_file(path);
        return Ok(());
    }
    crate::home::write_secure(
        &path,
        &serde_json::to_vec_pretty(&OutcomeNoticeRef {
            version: NOTICE_REF_VERSION,
            job_id: outcome.job_id.clone(),
            delivery_id: outcome.delivery_id.clone(),
            next_attempt_at: outcome.next_notification_attempt_at,
        })?,
    )?;
    Ok(())
}

fn reserve_execution(job_id: &str, delivery_id: &str) -> Result<bool> {
    let path = latch_path(job_id, delivery_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(&serde_json::to_vec_pretty(&ExecutionLatch {
                version: EXECUTION_LATCH_VERSION,
                job_id: job_id.to_string(),
                delivery_id: delivery_id.to_string(),
                phase: ExecutionPhase::Reserved,
                updated_at: now_secs(),
                direct_amount: None,
                direct_execution_mode: None,
            })?)?;
            file.sync_all()?;
            Ok(true)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn reserve_guide_direct_execution(job_id: &str, delivery_id: &str) -> Result<bool> {
    let path = latch_path(job_id, delivery_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(&serde_json::to_vec_pretty(&ExecutionLatch {
                version: EXECUTION_LATCH_VERSION,
                job_id: job_id.to_string(),
                delivery_id: delivery_id.to_string(),
                // Claim happens immediately before the external call. An
                // interruption is therefore always treated as non-retryable.
                phase: ExecutionPhase::Spawned,
                updated_at: now_secs(),
                direct_amount: None,
                direct_execution_mode: Some(ExecutionMode::Auto),
            })?)?;
            file.sync_all()?;
            Ok(true)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
fn update_execution_phase(job_id: &str, delivery_id: &str, phase: ExecutionPhase) -> Result<()> {
    let existing = read_execution_latch(job_id, delivery_id)?;
    let direct_amount = existing
        .as_ref()
        .and_then(|latch| latch.direct_amount.clone());
    let direct_execution_mode = existing
        .as_ref()
        .and_then(|latch| latch.direct_execution_mode);
    crate::home::write_secure(
        &latch_path(job_id, delivery_id)?,
        &serde_json::to_vec_pretty(&ExecutionLatch {
            version: EXECUTION_LATCH_VERSION,
            job_id: job_id.to_string(),
            delivery_id: delivery_id.to_string(),
            phase,
            updated_at: now_secs(),
            direct_amount,
            direct_execution_mode,
        })?,
    )?;
    Ok(())
}

fn read_execution_latch(job_id: &str, delivery_id: &str) -> Result<Option<ExecutionLatch>> {
    let path = latch_path(job_id, delivery_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let latch: ExecutionLatch = serde_json::from_slice(&std::fs::read(path)?)?;
    if !(1..=EXECUTION_LATCH_VERSION).contains(&latch.version)
        || latch.job_id != job_id
        || latch.delivery_id != delivery_id
    {
        bail!("execution latch mismatch");
    }
    Ok(Some(latch))
}

fn read_execution_phase(job_id: &str, delivery_id: &str) -> Result<Option<ExecutionPhase>> {
    Ok(read_execution_latch(job_id, delivery_id)?.map(|latch| latch.phase))
}

pub(crate) fn recovery_state(job_id: &str, delivery_id: &str) -> Result<RecoveryState> {
    if read_outcome(&outcome_path(job_id, delivery_id)?)?.is_some() {
        return Ok(RecoveryState::TerminalOutcome);
    }
    let path = latch_path(job_id, delivery_id)?;
    if !path.exists() {
        return Ok(RecoveryState::NoExecution);
    }
    match read_execution_phase(job_id, delivery_id) {
        Ok(Some(ExecutionPhase::Reserved | ExecutionPhase::Prepared)) => {
            Ok(RecoveryState::PreSubmitInterrupted)
        }
        Ok(Some(ExecutionPhase::Spawned)) | Err(_) => Ok(RecoveryState::SubmissionUnknown),
        Ok(None) => Ok(RecoveryState::NoExecution),
    }
}

fn read_outcome(path: &Path) -> Result<Option<ExecutionOutcome>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read(path)?;
    let outcome: ExecutionOutcome = serde_json::from_slice(&raw)?;
    if outcome.version != OUTCOME_VERSION {
        bail!("unsupported automatic execution outcome version");
    }
    Ok(Some(outcome))
}

fn write_outcome(path: &Path, outcome: &ExecutionOutcome) -> Result<()> {
    crate::home::write_secure(path, &serde_json::to_vec_pretty(outcome)?)?;
    if let Err(error) = sync_notice_ref(outcome) {
        // The durable execution outcome is authoritative. A secondary index
        // failure must never turn a submitted/unknown transaction into a CLI
        // error; explicit job-scoped flush can still recover from the outcome.
        eprintln!("[autotrade] pending-notification index update failed: {error}");
    }
    Ok(())
}

fn write_terminal_journal(outcome: &ExecutionOutcome) -> Result<PathBuf> {
    let path = terminal_journal_path(&outcome.job_id, &outcome.delivery_id)?;
    crate::home::write_secure(
        &path,
        &serde_json::to_vec_pretty(&TerminalJournal {
            version: TERMINAL_JOURNAL_VERSION,
            outcome: outcome.clone(),
        })?,
    )?;
    Ok(path)
}

fn read_terminal_journal(path: &Path) -> Result<TerminalJournal> {
    let journal: TerminalJournal = serde_json::from_slice(&std::fs::read(path)?)?;
    if journal.version != TERMINAL_JOURNAL_VERSION {
        bail!("unsupported terminal journal version");
    }
    Ok(journal)
}

fn terminal_reconciliation_complete(outcome: &ExecutionOutcome) -> bool {
    if sync_notice_ref(outcome).is_err() {
        return false;
    }
    if !super::delivery_queue::contains_delivery(&outcome.job_id, &outcome.delivery_id)
        .map(|present| !present)
        .unwrap_or(false)
    {
        return false;
    }
    sync_a2a_trade_record(outcome).is_ok()
}

fn sync_a2a_trade_record(outcome: &ExecutionOutcome) -> Result<()> {
    let context = consent::load_delivery_context(&outcome.job_id, &outcome.delivery_id)
        .context("trusted delivery context is unavailable for trade record")?;
    let status = serde_json::to_value(outcome.status)?;
    record_signal_status(
        &outcome.job_id,
        &outcome.delivery_id,
        status.as_str().unwrap_or("unknown"),
        outcome.reason.as_deref().unwrap_or(""),
        &context.saved_path,
    )
}

/// Write the current processing state of an A2A subscription Signal to the
/// local trade-record store. `extra` is the original Signal JSON itself, not a
/// nested wrapper, so its fields remain directly queryable. Plain-text Signals
/// use `extra.content` so they are recorded as well.
pub fn record_signal_status(
    job_id: &str,
    delivery_id: &str,
    status: &str,
    reason: &str,
    saved_path: &str,
) -> Result<()> {
    let raw_signal = std::fs::read_to_string(saved_path)
        .context("saved subscription Signal is unavailable for trade record")?;
    let extra = serde_json::from_str(&raw_signal)
        .unwrap_or_else(|_| serde_json::json!({ "content": raw_signal }));
    okx_a2a::trade_records_insert(&serde_json::json!([{
        "jobId": job_id,
        "deliveryId": delivery_id,
        "status": status,
        "reason": reason,
        "extra": extra,
    }]))
}

fn sensitive_label(value: &str) -> bool {
    let normalized = value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect::<String>();
    const NAMES: &[&str] = &[
        "apikey",
        "secret",
        "secretkey",
        "passphrase",
        "password",
        "authorization",
        "accesstoken",
        "refreshtoken",
        "token",
        "cookie",
        "signature",
        "privatekey",
        "mnemonic",
        "seed",
    ];
    NAMES
        .iter()
        .any(|name| normalized == *name || normalized.ends_with(name))
}

fn looks_like_jwt(value: &str) -> bool {
    let trimmed = value.trim_matches(|character: char| {
        matches!(character, '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']')
    });
    trimmed.len() >= 40
        && trimmed.bytes().filter(|byte| *byte == b'.').count() == 2
        && trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Reduce child-process output to a bounded, single-line, user-safe summary.
/// Raw stdout/stderr is never persisted. Known secret assignments, bearer
/// values, JWT-shaped values, and control characters are removed first.
fn safe_child_text(value: &str) -> String {
    let printable = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let mut output = Vec::new();
    let mut redact_next = 0usize;
    for token in printable.split_whitespace() {
        if redact_next > 0 {
            output.push("[REDACTED]".to_string());
            redact_next -= 1;
            continue;
        }
        if token.eq_ignore_ascii_case("bearer") {
            output.push("Bearer".to_string());
            redact_next = 1;
            continue;
        }
        if looks_like_jwt(token) {
            output.push("[REDACTED]".to_string());
            continue;
        }
        let assignment = token
            .char_indices()
            .filter(|(_, character)| matches!(character, '=' | ':'))
            .find(|(position, _)| sensitive_label(&token[..*position]))
            .map(|(position, separator)| (&token[..position], &token[position + 1..], separator));
        if let Some((label, assigned, separator)) = assignment {
            output.push(format!("{label}{separator}[REDACTED]"));
            if assigned.is_empty() {
                redact_next = if label.eq_ignore_ascii_case("authorization") {
                    2
                } else {
                    1
                };
            }
            continue;
        }
        if sensitive_label(token) {
            output.push(token.to_string());
            redact_next = 1;
            continue;
        }
        output.push(token.to_string());
    }
    safe_text(&output.join(" "))
}

fn trade_kit_authentication_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "failed to spawn okx-auth",
        "no credentials found",
        "not logged in",
        "not authenticated",
        "requires_auth",
        "session expired",
        "401 unauthorized",
        "http 401",
        "token refresh failed",
        "token expired",
        "token not found",
        "storagenotfounderror",
        "no config found",
        "run `okx auth login`",
        "run okx auth login",
        "run `okx config init`",
        "run okx config init",
        "re-run: okx config init",
        "api key doesn't exist",
        "api key does not exist",
        "invalid api-key",
        "invalid api key",
        "invalid ok-access-key",
        "invalid sign",
        "passphrase is incorrect",
        "50100",
        "50110",
        "50111",
        "50112",
        "50113",
    ];
    MARKERS.iter().any(|marker| message.contains(marker))
}

fn failure_category_for(
    venue: &str,
    status: OutcomeStatus,
    reason: Option<&str>,
) -> Option<FailureCategory> {
    if venue == "trade_kit"
        && status == OutcomeStatus::FailedBeforeSubmit
        && reason.is_some_and(trade_kit_authentication_error)
    {
        Some(FailureCategory::AuthenticationRequired)
    } else {
        None
    }
}

fn safe_text(value: &str) -> String {
    let mut reason = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if reason.is_empty() {
        reason = "unspecified terminal reason".to_string();
    }
    if reason.chars().count() > 240 {
        reason = reason.chars().take(240).collect::<String>() + "…";
    }
    reason
}

fn safe_metadata_token(value: &str, max_chars: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= max_chars
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':' | '/')
        })
}

pub(crate) async fn restore_subscription_local_contract(
    job_id: &str,
    agent_id: &str,
    provider_agent_id: &str,
    service_id: &str,
    service_hint: Option<&Value>,
) -> Result<Option<subscription_config::ExecutionMode>> {
    if !guide::guide_path(job_id)?.is_file() {
        let service = match service_hint {
            Some(service) => service.clone(),
            None => crate::commands::agent_commerce::task::common::find_service(
                provider_agent_id,
                service_id,
            )
            .await?
            .context("service is not available to restore its Guide")?,
        };
        let source = service
            .get("serviceGuide")
            .and_then(Value::as_str)
            .context("service has no Guide to restore")?;
        let source_hash = service.get("serviceGuideHash").and_then(Value::as_str);
        let draft = guide::parse_draft(Some(source), source_hash)?
            .context("service has no Guide to restore")?;
        let file = draft
            .clone()
            .into_file(job_id, service_id, Some(provider_agent_id));
        guide::write_guide(&file, &draft.source)?;
    }
    guide::migrate_legacy_json_consent_if_needed(job_id, agent_id, service_id)?;
    subscription_config::execution_mode(agent_id, service_id)
}

async fn hydrate_subscription_contract(
    job_id: &str,
    context: &consent::DeliveryContext,
) -> Result<subscription::ActiveSubscription> {
    use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

    let mut client = TaskApiClient::new();
    let active = subscription::determine_active_delivery(&mut client, job_id, &context.agent_id)
        .await
        .map_err(|_| anyhow::anyhow!("subscription is no longer Active"))?;
    if active.provider_agent_id != context.provider_agent_id {
        bail!("Active subscription no longer matches this delivery")
    }
    restore_subscription_local_contract(
        job_id,
        &context.agent_id,
        &context.provider_agent_id,
        &active.service_id,
        None,
    )
    .await?;
    Ok(active)
}

async fn require_guide_direct_subscription(
    job_id: &str,
    context: &consent::DeliveryContext,
) -> Result<()> {
    let active = hydrate_subscription_contract(job_id, context).await?;
    if subscription_config::execution_mode(&context.agent_id, &active.service_id)?
        != Some(subscription_config::ExecutionMode::GuideDirect)
    {
        bail!("automatic copy-trading is not enabled for this subscription")
    }
    Ok(())
}

/// Restore local Guide/Consent material for a saved Agent-direct delivery.
/// Unlike `claim_guide_direct`, this never creates an execution latch and must
/// not be treated as authorization to invoke a money-moving tool.
pub async fn prepare_guide_direct(job_id: &str, delivery_id: &str) -> Result<GuidePrepareResult> {
    use crate::commands::agent_commerce::task::common::config::SubscriptionTradePath;

    let context = consent::load_delivery_context(job_id, delivery_id)
        .context("trusted delivery context is unavailable")?;
    if context.execution_path != SubscriptionTradePath::AgentDirect {
        bail!("delivery is pinned to the legacy execution wrapper");
    }
    if !std::path::Path::new(&context.saved_path).is_file() {
        bail!("saved subscription Signal is not available")
    }
    let reason = match require_guide_direct_subscription(job_id, &context).await {
        Ok(()) if guide::has_active_execution_contract(job_id) => None,
        Ok(()) => Some("active local Service Guide and Guide Consent are required".to_string()),
        Err(error) => Some(error.to_string()),
    };
    Ok(GuidePrepareResult {
        ready: reason.is_none(),
        status: if reason.is_none() {
            "ready".to_string()
        } else {
            "not_ready".to_string()
        },
        job_id: job_id.to_string(),
        delivery_id: delivery_id.to_string(),
        reason,
    })
}

/// Claim a delivery for the Guide-driven flow. The runtime Agent determines the
/// tool call from the exact local Guide, Consent, and saved Signal; the CLI
/// verifies only the active Guide+Consent contract, the saved delivery, and
/// exactly-once admission.
pub async fn claim_guide_direct(job_id: &str, delivery_id: &str) -> Result<DirectClaimResult> {
    use crate::commands::agent_commerce::task::common::config::SubscriptionTradePath;

    let context = consent::load_delivery_context(job_id, delivery_id)
        .context("trusted delivery context is unavailable")?;
    if context.execution_path != SubscriptionTradePath::AgentDirect {
        bail!("delivery is pinned to the legacy execution wrapper");
    }
    if !std::path::Path::new(&context.saved_path).is_file() {
        bail!("saved subscription Signal is not available")
    }
    require_guide_direct_subscription(job_id, &context).await?;
    if !guide::has_active_execution_contract(job_id) {
        bail!("active local Service Guide and Guide Consent are required")
    }
    let path = outcome_path(job_id, delivery_id)?;
    if read_outcome(&path)?.is_some() {
        return Ok(DirectClaimResult {
            allowed: false,
            status: "terminal".to_string(),
            job_id: job_id.to_string(),
            delivery_id: delivery_id.to_string(),
            amount: None,
            reason: Some("delivery already has a terminal outcome".to_string()),
        });
    }
    if !reserve_guide_direct_execution(job_id, delivery_id)? {
        return Ok(DirectClaimResult {
            allowed: false,
            status: "already_claimed".to_string(),
            job_id: job_id.to_string(),
            delivery_id: delivery_id.to_string(),
            amount: None,
            reason: Some(
                "an earlier Guide-driven execution may have started; do not retry".to_string(),
            ),
        });
    }
    let result = DirectClaimResult {
        allowed: true,
        status: "claimed".to_string(),
        job_id: job_id.to_string(),
        delivery_id: delivery_id.to_string(),
        amount: None,
        reason: None,
    };
    // Do not reverse a successfully persisted execution reservation when the
    // auxiliary local trade-record store is temporarily unavailable. The
    // `received` record remains until a later terminal update replaces it.
    let _ = record_signal_status(job_id, delivery_id, &result.status, "", &context.saved_path);
    Ok(result)
}

/// Persist a selected Skill/tool's documented terminal result. The coordinator
/// accepts only bounded metadata; it never parses, rewrites, or executes the
/// target command and never attempts a cross-path fallback.
pub fn finalize_direct(
    job_id: &str,
    delivery_id: &str,
    status: &str,
    tool_id: &str,
    receipt_id: Option<&str>,
    reason: Option<&str>,
) -> Result<ExecutionOutcome> {
    use crate::commands::agent_commerce::task::common::config::SubscriptionTradePath;

    let context = consent::load_delivery_context(job_id, delivery_id)
        .context("trusted delivery context is unavailable")?;
    if context.execution_path != SubscriptionTradePath::AgentDirect {
        bail!("delivery is pinned to the legacy execution wrapper");
    }
    if !safe_metadata_token(tool_id, 128) {
        bail!("direct execution tool id is invalid");
    }
    let path = outcome_path(job_id, delivery_id)?;
    if let Some(mut outcome) = read_outcome(&path)? {
        if outcome.notification_pending {
            notify_and_persist(&path, &mut outcome, false, Some(INITIAL_NOTIFY_TIMEOUT));
        }
        return Ok(outcome);
    }
    let latch =
        read_execution_latch(job_id, delivery_id)?.context("direct execution was not claimed")?;
    // New Guide-direct claims do not receive a coordinator-owned amount. Keep
    // an amount written by an older latch for backward-compatible outcomes.
    let amount = latch.direct_amount.unwrap_or_default();
    let execution_mode = latch
        .direct_execution_mode
        .context("direct execution claim mode is unavailable")?;
    let (status, receipt, reason) = match status {
        "submitted" => {
            let receipt_id = receipt_id.context("submitted direct execution requires a receipt id")?;
            if !safe_metadata_token(receipt_id, 256) {
                bail!("direct execution receipt id is invalid");
            }
            (
                OutcomeStatus::Submitted,
                Some(serde_json::json!({ "receiptId": receipt_id })),
                None,
            )
        }
        "failed_before_submit" => (
            OutcomeStatus::FailedBeforeSubmit,
            None,
            Some(safe_child_text(
                reason.unwrap_or("selected tool rejected the trade before submission"),
            )),
        ),
        "unknown_after_submit" => (
            OutcomeStatus::UnknownAfterSubmit,
            None,
            Some(safe_child_text(
                reason.unwrap_or("selected tool returned an unknown submission state"),
            )),
        ),
        _ => bail!(
            "direct execution status must be submitted, failed_before_submit, or unknown_after_submit"
        ),
    };
    let failure_category = if tool_id == "trade_kit" {
        failure_category_for("trade_kit", status, reason.as_deref())
    } else {
        None
    };
    let now = now_secs();
    persist_and_notify(
        &path,
        ExecutionOutcome {
            version: OUTCOME_VERSION,
            job_id: job_id.to_string(),
            delivery_id: delivery_id.to_string(),
            venue: format!("agent_direct/{tool_id}"),
            action: "execute".to_string(),
            amount,
            execution_mode,
            status,
            receipt,
            reason,
            failure_category,
            notification_pending: true,
            notification_attempts: 0,
            next_notification_attempt_at: 0,
            created_at: now,
            updated_at: now,
        },
    )
}

fn persist_and_notify(path: &Path, mut outcome: ExecutionOutcome) -> Result<ExecutionOutcome> {
    let journal_path = write_terminal_journal(&outcome)
        .map(Some)
        .unwrap_or_else(|error| {
            eprintln!("[autotrade] terminal journal write failed: {error}");
            None
        });
    write_outcome(path, &outcome)?;
    consent::clear_pending_delivery(&outcome.job_id, &outcome.delivery_id);
    notify_and_persist(path, &mut outcome, false, Some(INITIAL_NOTIFY_TIMEOUT));
    // A durable terminal result, including a user-selected skip, is the only
    // normal trigger that advances the per-subscription decision FIFO.
    if let Err(error) =
        super::delivery_queue::complete_and_advance(&outcome.job_id, &outcome.delivery_id)
    {
        eprintln!("[autotrade] queued-delivery resume failed (persisted for retry): {error}");
    }
    if terminal_reconciliation_complete(&outcome) {
        if let Some(journal_path) = journal_path {
            let _ = std::fs::remove_file(journal_path);
        }
    }
    Ok(outcome)
}

fn notification(outcome: &ExecutionOutcome) -> String {
    let receipt = outcome
        .receipt
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|map| {
            map.iter()
                .find_map(|(key, value)| value.as_str().map(|v| format!("{key}: {v}")))
        });
    let zh_label = match outcome.execution_mode {
        ExecutionMode::Auto => "[自动跟单]",
        ExecutionMode::Manual | ExecutionMode::OneTime => "[手动跟单]",
    };
    let en_label = match outcome.execution_mode {
        ExecutionMode::Auto => "[Auto Copy-Trade]",
        ExecutionMode::Manual | ExecutionMode::OneTime => "[Manual Copy-Trade]",
    };
    let zh_auth_guidance =
        if outcome.failure_category == Some(FailureCategory::AuthenticationRequired) {
            " 请回复“连接 Trade Kit”以启动授权；授权完成后，本次交易不会自动重试。"
        } else {
            ""
        };
    let en_auth_guidance = if outcome.failure_category
        == Some(FailureCategory::AuthenticationRequired)
    {
        " Reply “Connect Trade Kit” to start authorization. This trade will not be retried automatically after authorization."
    } else {
        ""
    };
    match (user_lang::resolve(&outcome.job_id), outcome.status) {
        (user_lang::Lang::Zh, OutcomeStatus::Submitted) => format!(
            "{zh_label} 交易已提交。类型: {},方向: {},金额: {}。{}",
            outcome.venue,
            outcome.action,
            outcome.amount,
            receipt.unwrap_or_else(|| "可在对应交易记录中查看详情".to_string())
        ),
        (user_lang::Lang::En, OutcomeStatus::Submitted) => format!(
            "{en_label} Trade submitted. Venue: {}, action: {}, amount: {}. {}",
            outcome.venue,
            outcome.action,
            outcome.amount,
            receipt.unwrap_or_else(|| "Check the venue history for details".to_string())
        ),
        (user_lang::Lang::Zh, OutcomeStatus::FailedBeforeSubmit) => format!(
            "{zh_label} 交易执行失败，未确认提交。类型: {},方向: {},金额: {}。原因: {}。系统不会自动重试。{zh_auth_guidance}",
            outcome.venue,
            outcome.action,
            outcome.amount,
            outcome.reason.as_deref().unwrap_or("执行命令失败")
        ),
        (user_lang::Lang::En, OutcomeStatus::FailedBeforeSubmit) => format!(
            "{en_label} Trade execution failed; submission was not confirmed. Venue: {}, action: {}, amount: {}. Reason: {}. No automatic retry will occur.{en_auth_guidance}",
            outcome.venue,
            outcome.action,
            outcome.amount,
            outcome.reason.as_deref().unwrap_or("execution command failed")
        ),
        (user_lang::Lang::Zh, OutcomeStatus::UnknownAfterSubmit) => format!(
            "{zh_label} 交易提交状态未知。类型: {},方向: {},金额: {}。原因: {}。请先查询订单/交易记录，系统不会自动重试。",
            outcome.venue,
            outcome.action,
            outcome.amount,
            outcome.reason.as_deref().unwrap_or("未获得可验证的交易回执")
        ),
        (user_lang::Lang::En, OutcomeStatus::UnknownAfterSubmit) => format!(
            "{en_label} Trade submission status is unknown. Venue: {}, action: {}, amount: {}. Reason: {}. Check order/transaction history first; no automatic retry will occur.",
            outcome.venue,
            outcome.action,
            outcome.amount,
            outcome.reason.as_deref().unwrap_or("no verifiable transaction receipt was returned")
        ),
        (user_lang::Lang::Zh, OutcomeStatus::Skipped) => format!(
            "{zh_label} {}",
            if outcome.reason.as_deref()
                == Some(super::GUIDE_EXECUTION_UNAVAILABLE_REASON)
            {
                "本次 Signal 已保存，仅接收和展示：该订阅没有有效的本地 Service Guide 与 Guide Consent 执行合约。不会提交订单，旧 execution Consent 不会生效。"
                    .to_string()
            } else if outcome.reason.as_deref()
                == Some(super::EXECUTION_POLICY_NOT_CONFIGURED_REASON)
            {
                "本次交付物已保存并跳过：固定字段的旧执行策略已退役，不会创建或恢复执行配置。"
                    .to_string()
            } else {
                format!(
                    "本次交付物未执行交易。原因: {}。",
                    outcome.reason.as_deref().unwrap_or("信号不满足执行条件")
                )
            }
        ),
        (user_lang::Lang::En, OutcomeStatus::Skipped) => format!(
            "{en_label} {}",
            if outcome.reason.as_deref()
                == Some(super::GUIDE_EXECUTION_UNAVAILABLE_REASON)
            {
                "The Signal was saved for receiving/display only: this subscription has no valid local Service Guide + active Guide Consent execution contract. No order was submitted and legacy execution Consent does not apply."
                    .to_string()
            } else if outcome.reason.as_deref()
                == Some(super::EXECUTION_POLICY_NOT_CONFIGURED_REASON)
            {
                "The deliverable was saved and skipped: the fixed-field legacy execution policy is retired and cannot create or restore execution configuration."
                    .to_string()
            } else {
                format!(
                    "No trade was executed for this delivery. Reason: {}.",
                    outcome.reason.as_deref().unwrap_or("the signal was not eligible for execution")
                )
            }
        ),
        (user_lang::Lang::Zh, OutcomeStatus::FailedBeforeExecution) => format!(
            "{zh_label} 交付物处理失败，未启动交易。原因: {}。系统不会自动下单重试。",
            outcome.reason.as_deref().unwrap_or("无法完成交易前处理").trim_end_matches('。')
        ),
        (user_lang::Lang::En, OutcomeStatus::FailedBeforeExecution) => format!(
            "{en_label} Delivery processing failed before a trade was started. Reason: {}. No automatic order retry will occur.",
            outcome.reason.as_deref().unwrap_or("pre-trade processing could not be completed")
        ),
    }
}

fn notify_and_persist(
    path: &Path,
    outcome: &mut ExecutionOutcome,
    force: bool,
    attempt_timeout: Option<Duration>,
) {
    if !outcome.notification_pending
        || (!force && outcome.next_notification_attempt_at > now_secs())
    {
        return;
    }
    let key = format!(
        "autotrade-outcome:{}",
        hex::encode(Sha256::digest(format!(
            "{}\0{}\0{:?}",
            outcome.job_id, outcome.delivery_id, outcome.status
        )))
    );
    let content = notification(outcome);
    let max_attempts = if force { 3 } else { 1 };
    for attempt in 0..max_attempts {
        let delivered = match attempt_timeout {
            Some(timeout) => {
                okx_a2a::user_notify_scoped_with_timeout(&content, &outcome.job_id, &key, timeout)
            }
            None => okx_a2a::user_notify_scoped(&content, &outcome.job_id, &key),
        };
        if delivered.is_ok() {
            outcome.notification_pending = false;
            outcome.next_notification_attempt_at = 0;
            outcome.updated_at = now_secs();
            let _ = write_outcome(path, outcome);
            return;
        }
        if attempt + 1 < max_attempts {
            std::thread::sleep(Duration::from_millis(50 * (attempt + 1)));
        }
    }
    outcome.notification_attempts = outcome
        .notification_attempts
        .saturating_add(max_attempts as u32);
    outcome.updated_at = now_secs();
    if outcome.notification_attempts >= MAX_NOTIFICATION_ATTEMPTS {
        outcome.notification_pending = false;
        outcome.next_notification_attempt_at = 0;
        let _ = write_outcome(path, outcome);
        return;
    }
    let delay = 30u64
        .saturating_mul(1u64 << outcome.notification_attempts.min(5))
        .min(15 * 60);
    outcome.next_notification_attempt_at = outcome.updated_at.saturating_add(delay);
    let _ = write_outcome(path, outcome);
}

/// Persist and notify a terminal delivery result that occurs before a
/// money-moving command exists (for example, a non-actionable signal or route
/// preparation failure). Reserving the same delivery latch prevents a later
/// model turn from executing a delivery it already declared terminal.
pub fn report_delivery(
    job_id: &str,
    delivery_id: &str,
    status: &str,
    reason: &str,
) -> Result<ExecutionOutcome> {
    let context = consent::load_delivery_context(job_id, delivery_id)
        .context("trusted delivery context is unavailable")?;
    if context.job_id != job_id || context.delivery_id != delivery_id {
        bail!("trusted delivery context mismatch");
    }
    let mut status = match status {
        "skipped" => OutcomeStatus::Skipped,
        "failed_before_execution" => OutcomeStatus::FailedBeforeExecution,
        _ => bail!("delivery report status must be skipped or failed_before_execution"),
    };
    // Older agents can still resume a persisted AgentDirect context after its
    // Guide Consent disappeared. That is no longer a failed delivery: the
    // Signal remains useful, but execution is disabled. Normalize both that
    // stale context and the retired error wording before any latch/outcome is
    // written so the user never sees "No active execution consent" again.
    let contract_unavailable = matches!(
        context.execution_path,
        crate::commands::agent_commerce::task::common::config::SubscriptionTradePath::AgentDirect
    ) && !guide::has_active_execution_contract(job_id);
    let reason = if contract_unavailable || is_retired_execution_consent_reason(reason) {
        status = OutcomeStatus::Skipped;
        super::GUIDE_EXECUTION_UNAVAILABLE_REASON.to_string()
    } else {
        safe_text(reason)
    };
    let outcome_path = outcome_path(job_id, delivery_id)?;
    // Terminal history is authoritative, including records from releases that
    // did not persist a latch. A later report cannot normalize or replace it.
    if let Some(mut outcome) = read_outcome(&outcome_path)? {
        if outcome.notification_pending {
            notify_and_persist(
                &outcome_path,
                &mut outcome,
                false,
                Some(INITIAL_NOTIFY_TIMEOUT),
            );
        }
        return Ok(outcome);
    }
    if !reserve_execution(job_id, delivery_id)? {
        // The Guide claim and this report use the same atomic reservation.
        // Preserve a terminal result that won the race; claimed or uncertain
        // work cannot be closed as a pre-execution skip.
        if let Some(mut outcome) = read_outcome(&outcome_path)? {
            if outcome.notification_pending {
                notify_and_persist(
                    &outcome_path,
                    &mut outcome,
                    false,
                    Some(INITIAL_NOTIFY_TIMEOUT),
                );
            }
            return Ok(outcome);
        }
        bail!("delivery already reserved without a terminal outcome");
    }
    let now = now_secs();
    persist_and_notify(
        &outcome_path,
        ExecutionOutcome {
            version: OUTCOME_VERSION,
            job_id: job_id.to_string(),
            delivery_id: delivery_id.to_string(),
            venue: String::new(),
            action: String::new(),
            amount: String::new(),
            execution_mode: ExecutionMode::Auto,
            status,
            receipt: None,
            reason: Some(reason),
            failure_category: None,
            notification_pending: true,
            notification_attempts: 0,
            next_notification_attempt_at: 0,
            created_at: now,
            updated_at: now,
        },
    )
}

fn is_retired_execution_consent_reason(reason: &str) -> bool {
    let normalized = reason.to_ascii_lowercase();
    normalized.contains("active execution consent")
        || normalized.contains("automatic execution consent")
        || normalized.contains("copy-trade consent")
        || normalized.contains("auto-trade consent")
}

/// Convert an abandoned execution gateway invocation into a durable terminal
/// result without ever starting the target command again.
pub(crate) fn recover_incomplete(job_id: &str, delivery_id: &str) -> Result<bool> {
    let state = recovery_state(job_id, delivery_id)?;
    if state == RecoveryState::NoExecution {
        return Ok(false);
    }
    if state == RecoveryState::TerminalOutcome {
        let path = outcome_path(job_id, delivery_id)?;
        if let Some(outcome) = read_outcome(&path)? {
            write_outcome(&path, &outcome)?;
            consent::clear_pending_delivery(job_id, delivery_id);
            let _ = super::delivery_queue::reconcile_terminal(job_id, delivery_id);
        }
        return Ok(true);
    }
    let (status, reason) = match state {
        RecoveryState::PreSubmitInterrupted => (
            OutcomeStatus::FailedBeforeSubmit,
            "execution was interrupted before the transaction command started; no order was submitted and no automatic retry will occur",
        ),
        RecoveryState::SubmissionUnknown => (
            OutcomeStatus::UnknownAfterSubmit,
            "execution was interrupted after the transaction command may have started; submission state is unknown and no automatic retry will occur",
        ),
        RecoveryState::NoExecution | RecoveryState::TerminalOutcome => unreachable!(),
    };
    let now = now_secs();
    persist_and_notify(
        &outcome_path(job_id, delivery_id)?,
        ExecutionOutcome {
            version: OUTCOME_VERSION,
            job_id: job_id.to_string(),
            delivery_id: delivery_id.to_string(),
            venue: String::new(),
            action: String::new(),
            amount: String::new(),
            execution_mode: ExecutionMode::Auto,
            status,
            receipt: None,
            reason: Some(reason.to_string()),
            failure_category: None,
            notification_pending: true,
            notification_attempts: 0,
            next_notification_attempt_at: 0,
            created_at: now,
            updated_at: now,
        },
    )?;
    Ok(true)
}

/// Repair terminal transitions that were interrupted between outcome, notice
/// index, pending pointer, and FIFO updates. This never invokes a trade command.
pub fn reconcile_terminal_journals(max_records: usize, budget: Duration) -> Result<usize> {
    let deadline = Instant::now() + budget;
    let root = crate::home::onchainos_home()?
        .join("autotrade")
        .join("terminal-journal");
    if !root.is_dir() {
        return Ok(0);
    }
    let mut repaired = 0;
    'jobs: for directory in std::fs::read_dir(root)? {
        let directory = directory?;
        if !directory.path().is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(directory.path())? {
            if repaired >= max_records || Instant::now() >= deadline {
                break 'jobs;
            }
            let path = entry?.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let journal = match read_terminal_journal(&path) {
                Ok(journal) => journal,
                Err(error) => {
                    eprintln!(
                        "[autotrade] unreadable terminal journal {:?}: {error}",
                        path
                    );
                    continue;
                }
            };
            let outcome_path = outcome_path(&journal.outcome.job_id, &journal.outcome.delivery_id)?;
            let outcome = read_outcome(&outcome_path)?.unwrap_or(journal.outcome);
            write_outcome(&outcome_path, &outcome)?;
            consent::clear_pending_delivery(&outcome.job_id, &outcome.delivery_id);
            let _ =
                super::delivery_queue::reconcile_terminal(&outcome.job_id, &outcome.delivery_id);
            if terminal_reconciliation_complete(&outcome) {
                let _ = std::fs::remove_file(&path);
            }
            repaired += 1;
        }
    }
    Ok(repaired)
}

fn flush_with_policy(
    job_id: &str,
    force: bool,
    max_records: usize,
) -> Result<Vec<ExecutionOutcome>> {
    if !grants::job_id_is_safe(job_id) {
        bail!("invalid job id");
    }
    let directory = crate::home::onchainos_home()?
        .join("autotrade")
        .join("outcomes")
        .join(job_id);
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut outcomes = Vec::new();
    for entry in std::fs::read_dir(directory)?.take(max_records.max(1)) {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Some(mut outcome) = read_outcome(&path)? else {
            continue;
        };
        if outcome.notification_pending {
            notify_and_persist(&path, &mut outcome, force, None);
        }
        outcomes.push(outcome);
    }
    let _ = super::notify::flush_pending(job_id, force);
    Ok(outcomes)
}

pub fn flush(job_id: &str) -> Result<Vec<ExecutionOutcome>> {
    flush_with_policy(job_id, true, 32)
}

pub fn flush_due(job_id: &str) -> Result<Vec<ExecutionOutcome>> {
    if !grants::job_id_is_safe(job_id) {
        bail!("invalid job id");
    }
    let _ = flush_all_due(4)?;
    Ok(Vec::new())
}

/// Retry due result/degrade notices across jobs. Called opportunistically on
/// Agent command startup/heartbeat; it never re-runs a transaction command.
pub fn flush_all_due(max_records: usize) -> Result<usize> {
    let root = notice_index_root()?;
    let mut pending = Vec::new();
    if root.is_dir() {
        for entry in std::fs::read_dir(&root)? {
            let path = entry?.path();
            let extension = path.extension().and_then(|value| value.to_str());
            if extension.is_some_and(|value| value.starts_with("lease-")) {
                let stale = path
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                    .is_some_and(|age| age.as_secs() >= STALE_LEASE_SEC);
                if stale {
                    let original = path.with_extension("json");
                    if original.exists() {
                        let _ = std::fs::remove_file(&path);
                    } else {
                        let _ = std::fs::rename(&path, original);
                    }
                }
                continue;
            }
            if extension != Some("json") {
                continue;
            }
            if let Some(reference) = std::fs::read(&path)
                .ok()
                .and_then(|raw| serde_json::from_slice::<OutcomeNoticeRef>(&raw).ok())
                .filter(|reference| reference.version == NOTICE_REF_VERSION)
            {
                pending.push((path, reference));
            }
        }
        pending.sort_by_key(|(_, reference)| reference.next_attempt_at);
        for (index_path, reference) in pending.into_iter().take(max_records.max(1)) {
            if reference.next_attempt_at > now_secs() {
                break;
            }
            let lease = index_path.with_extension(format!("lease-{}", std::process::id()));
            if std::fs::rename(&index_path, &lease).is_err() {
                continue;
            }
            let path = match outcome_path(&reference.job_id, &reference.delivery_id) {
                Ok(path) => path,
                Err(_) => {
                    let _ = std::fs::remove_file(&lease);
                    continue;
                }
            };
            match read_outcome(&path) {
                Ok(Some(mut outcome)) if outcome.notification_pending => {
                    notify_and_persist(&path, &mut outcome, false, Some(FLUSH_NOTIFY_TIMEOUT));
                }
                _ => {
                    let _ = std::fs::remove_file(&index_path);
                }
            }
            // `write_outcome` recreated the canonical pending ref on failure;
            // success removed it. The lease itself is always disposable now.
            let _ = std::fs::remove_file(&lease);
        }
    }
    let delivered =
        super::notify::flush_all_pending_bounded(max_records, FLUSH_NOTIFY_TIMEOUT).unwrap_or(0);
    Ok(delivered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tempdir() -> tempfile::TempDir {
        let root = std::env::current_dir()
            .expect("current directory")
            .join("target")
            .join("autotrade-executor-test-home");
        std::fs::create_dir_all(&root).expect("create executor test root");
        tempfile::tempdir_in(root).expect("create executor test home")
    }

    #[test]
    fn pending_only_notification_index_tracks_outcome_state() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        let path = outcome_path("job1", "delivery-index").unwrap();
        let mut outcome = ExecutionOutcome {
            version: OUTCOME_VERSION,
            job_id: "job1".to_string(),
            delivery_id: "delivery-index".to_string(),
            venue: "dex".to_string(),
            action: "buy".to_string(),
            amount: "1".to_string(),
            execution_mode: ExecutionMode::Auto,
            status: OutcomeStatus::FailedBeforeSubmit,
            receipt: None,
            reason: Some("test".to_string()),
            failure_category: None,
            notification_pending: true,
            notification_attempts: 1,
            next_notification_attempt_at: now_secs() + 30,
            created_at: now_secs(),
            updated_at: now_secs(),
        };
        write_outcome(&path, &outcome).unwrap();
        let index = notice_ref_path("job1", "delivery-index").unwrap();
        assert!(index.exists());
        outcome.notification_pending = false;
        write_outcome(&path, &outcome).unwrap();
        assert!(!index.exists());
        assert!(
            path.exists(),
            "terminal outcome remains an idempotency tombstone"
        );
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn outcome_notification_stops_after_max_attempts() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        let empty_path = temp.path().join("empty-path");
        std::fs::create_dir_all(&empty_path).unwrap();
        let original_path = std::env::var_os("PATH");
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        std::env::set_var("PATH", &empty_path);

        let path = outcome_path("job-max-attempts", "delivery-1").unwrap();
        let mut outcome = ExecutionOutcome {
            version: OUTCOME_VERSION,
            job_id: "job-max-attempts".to_string(),
            delivery_id: "delivery-1".to_string(),
            venue: "dex".to_string(),
            action: "buy".to_string(),
            amount: "1".to_string(),
            execution_mode: ExecutionMode::Auto,
            status: OutcomeStatus::FailedBeforeSubmit,
            receipt: None,
            reason: Some("test".to_string()),
            failure_category: None,
            notification_pending: true,
            notification_attempts: MAX_NOTIFICATION_ATTEMPTS - 1,
            next_notification_attempt_at: 0,
            created_at: now_secs(),
            updated_at: now_secs(),
        };
        write_outcome(&path, &outcome).unwrap();
        assert!(notice_ref_path("job-max-attempts", "delivery-1")
            .unwrap()
            .exists());

        notify_and_persist(&path, &mut outcome, false, Some(Duration::from_millis(1)));

        let stored = read_outcome(&path).unwrap().unwrap();
        assert!(!stored.notification_pending);
        assert_eq!(stored.next_notification_attempt_at, 0);
        assert!(!notice_ref_path("job-max-attempts", "delivery-1")
            .unwrap()
            .exists());
        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn unconfigured_legacy_policy_skip_notification_does_not_offer_restore() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        user_lang::record_from_user_text("job-unconfigured-notice", "update this subscription");
        let outcome = ExecutionOutcome {
            version: OUTCOME_VERSION,
            job_id: "job-unconfigured-notice".to_string(),
            delivery_id: "delivery-1".to_string(),
            venue: String::new(),
            action: String::new(),
            amount: String::new(),
            execution_mode: ExecutionMode::Auto,
            status: OutcomeStatus::Skipped,
            receipt: None,
            reason: Some(super::super::EXECUTION_POLICY_NOT_CONFIGURED_REASON.to_string()),
            failure_category: None,
            notification_pending: true,
            notification_attempts: 0,
            next_notification_attempt_at: 0,
            created_at: now_secs(),
            updated_at: now_secs(),
        };

        let rendered = notification(&outcome);
        assert!(rendered.contains("deliverable was saved"));
        assert!(rendered.contains("fixed-field legacy execution policy is retired"));
        assert!(rendered.contains("cannot create or restore execution configuration"));
        assert!(!rendered.contains("Update this subscription's copy-trade execution policy"));
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn trade_kit_business_rejection_does_not_request_authentication() {
        assert_eq!(
            failure_category_for(
                "trade_kit",
                OutcomeStatus::FailedBeforeSubmit,
                Some("51008: insufficient account balance")
            ),
            None
        );
    }

    #[test]
    fn child_diagnostics_redact_sensitive_assignments_and_jwt_shaped_values() {
        let jwt_shaped = format!("{}.{}.{}", "a".repeat(16), "b".repeat(16), "c".repeat(16));
        let raw = format!(
            "apiKey=fixture-api-value --secret fixture-secret-value https://invalid.local?token=fixture-query-value {jwt_shaped} Error: denied"
        );
        let safe = safe_child_text(&raw);
        assert!(!safe.contains("fixture-api-value"));
        assert!(!safe.contains("fixture-secret-value"));
        assert!(!safe.contains("fixture-query-value"));
        assert!(!safe.contains(&jwt_shaped));
        assert!(safe.contains("[REDACTED]"));
        assert!(safe.contains("denied"));
    }

    #[test]
    fn execution_latch_distinguishes_pre_submit_from_spawned_recovery() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        assert_eq!(
            recovery_state("job-phase", "delivery-1").unwrap(),
            RecoveryState::NoExecution
        );
        assert!(reserve_execution("job-phase", "delivery-1").unwrap());
        assert_eq!(
            recovery_state("job-phase", "delivery-1").unwrap(),
            RecoveryState::PreSubmitInterrupted
        );
        update_execution_phase("job-phase", "delivery-1", ExecutionPhase::Spawned).unwrap();
        assert_eq!(
            recovery_state("job-phase", "delivery-1").unwrap(),
            RecoveryState::SubmissionUnknown
        );
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn guide_reservation_is_exactly_once_and_finalize_preserves_terminal_result() {
        use crate::commands::agent_commerce::task::common::config::SubscriptionTradePath;

        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        let signal = temp.path().join("signal.json");
        std::fs::write(&signal, r#"{"signal":"current Guide delivery"}"#).unwrap();
        consent::register_delivery_context_with_path(
            "job-guide-reservation",
            "7",
            "8",
            None,
            "delivery-1",
            signal.to_str().unwrap(),
            "json",
            1,
            SubscriptionTradePath::AgentDirect,
        )
        .unwrap();
        // Exercise the atomic helper used after authenticated Active and local
        // Guide/Consent verification in claim_guide_direct. No backend admission
        // or authorization verification is implied by this local test.
        assert!(reserve_guide_direct_execution("job-guide-reservation", "delivery-1").unwrap());
        let latch_path = latch_path("job-guide-reservation", "delivery-1").unwrap();
        let original_latch = std::fs::read(&latch_path).unwrap();
        assert!(!reserve_guide_direct_execution("job-guide-reservation", "delivery-1").unwrap());
        assert_eq!(std::fs::read(&latch_path).unwrap(), original_latch);
        let latch = read_execution_latch("job-guide-reservation", "delivery-1")
            .unwrap()
            .unwrap();
        assert_eq!(latch.phase, ExecutionPhase::Spawned);
        assert_eq!(latch.direct_amount, None);
        assert_eq!(latch.direct_execution_mode, Some(ExecutionMode::Auto));
        assert_eq!(
            recovery_state("job-guide-reservation", "delivery-1").unwrap(),
            RecoveryState::SubmissionUnknown
        );

        // Notification and trade-record dependencies are deliberately absent.
        // Finalization must remain durable and retain pending reconciliation.
        let empty_bin = temp.path().join("empty-bin");
        std::fs::create_dir_all(&empty_bin).unwrap();
        let original_path = std::env::var_os("PATH");
        std::env::set_var("PATH", &empty_bin);
        let submitted = finalize_direct(
            "job-guide-reservation",
            "delivery-1",
            "submitted",
            "fixture-tool",
            Some("order:guide123"),
            None,
        );
        let replay = finalize_direct(
            "job-guide-reservation",
            "delivery-1",
            "unknown_after_submit",
            "fixture-tool",
            None,
            Some("stale duplicate finalization"),
        );
        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        let submitted = submitted.unwrap();
        let replay = replay.unwrap();
        assert_eq!(submitted.status, OutcomeStatus::Submitted);
        assert_eq!(submitted.execution_mode, ExecutionMode::Auto);
        assert_eq!(submitted.amount, "");
        assert_eq!(replay.status, OutcomeStatus::Submitted);
        assert_eq!(replay.receipt.unwrap()["receiptId"], "order:guide123");
        assert_eq!(
            recovery_state("job-guide-reservation", "delivery-1").unwrap(),
            RecoveryState::TerminalOutcome
        );
        assert_eq!(std::fs::read(latch_path).unwrap(), original_latch);
        assert!(signal.exists());
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn legacy_latches_retain_amount_and_mode_without_new_authorization() {
        use crate::commands::agent_commerce::task::common::config::SubscriptionTradePath;

        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        for (delivery_id, mode) in [
            ("delivery-manual", ExecutionMode::Manual),
            ("delivery-one-time", ExecutionMode::OneTime),
        ] {
            consent::register_delivery_context_with_path(
                "job-history",
                "7",
                "8",
                None,
                delivery_id,
                "/tmp/signal.txt",
                "text",
                1,
                SubscriptionTradePath::AgentDirect,
            )
            .unwrap();
            // This is an existing historical claim, not a new authorization.
            let latch = ExecutionLatch {
                version: 1,
                job_id: "job-history".to_string(),
                delivery_id: delivery_id.to_string(),
                phase: ExecutionPhase::Spawned,
                updated_at: 1,
                direct_amount: Some("10".to_string()),
                direct_execution_mode: Some(mode),
            };
            crate::home::write_secure(
                &latch_path("job-history", delivery_id).unwrap(),
                &serde_json::to_vec(&latch).unwrap(),
            )
            .unwrap();
            assert_eq!(
                recovery_state("job-history", delivery_id).unwrap(),
                RecoveryState::SubmissionUnknown
            );
            let outcome = finalize_direct(
                "job-history",
                delivery_id,
                "submitted",
                "fixture-tool",
                Some("order:123"),
                None,
            )
            .unwrap();
            assert_eq!(outcome.amount, "10");
            assert_eq!(outcome.execution_mode, mode);
            assert_eq!(outcome.status, OutcomeStatus::Submitted);
            let replay = finalize_direct(
                "job-history",
                delivery_id,
                "failed_before_submit",
                "fixture-tool",
                None,
                Some("stale replay"),
            )
            .unwrap();
            assert_eq!(replay.status, OutcomeStatus::Submitted);
            assert_eq!(replay.receipt.unwrap()["receiptId"], "order:123");
            assert!(latch_path("job-history", delivery_id).unwrap().exists());
        }
        assert!(!temp.path().join("autotrade/one-time-permits").exists());
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn delivery_report_preserves_terminal_history_even_without_a_latch() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        for (delivery_id, status) in [
            ("delivery-submitted", OutcomeStatus::Submitted),
            ("delivery-unknown", OutcomeStatus::UnknownAfterSubmit),
            ("delivery-failed", OutcomeStatus::FailedBeforeExecution),
        ] {
            consent::register_delivery_context(
                "job-terminal-history",
                "7",
                "8",
                None,
                delivery_id,
                "/tmp/signal.txt",
                "text",
                1,
            )
            .unwrap();
            let path = outcome_path("job-terminal-history", delivery_id).unwrap();
            let outcome = ExecutionOutcome {
                version: OUTCOME_VERSION,
                job_id: "job-terminal-history".to_string(),
                delivery_id: delivery_id.to_string(),
                venue: "historical-tool".to_string(),
                action: "buy".to_string(),
                amount: "10".to_string(),
                execution_mode: ExecutionMode::OneTime,
                status,
                receipt: Some(serde_json::json!({"receiptId":"original:123"})),
                reason: Some("No active execution consent".to_string()),
                failure_category: None,
                notification_pending: false,
                notification_attempts: 0,
                next_notification_attempt_at: 0,
                created_at: 1,
                updated_at: 1,
            };
            write_outcome(&path, &outcome).unwrap();
            let before = std::fs::read(&path).unwrap();
            let result = report_delivery(
                "job-terminal-history",
                delivery_id,
                "skipped",
                "old decision",
            )
            .unwrap();
            assert_eq!(result.status, status);
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert!(!latch_path("job-terminal-history", delivery_id)
                .unwrap()
                .exists());
        }
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn delivery_report_rejects_claimed_and_uncertain_work() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        for delivery_id in ["delivery-claimed", "delivery-malformed"] {
            consent::register_delivery_context(
                "job-reserved",
                "7",
                "8",
                None,
                delivery_id,
                "/tmp/signal.txt",
                "text",
                1,
            )
            .unwrap();
            if delivery_id == "delivery-claimed" {
                assert!(reserve_guide_direct_execution("job-reserved", delivery_id).unwrap());
            } else {
                crate::home::write_secure(
                    &latch_path("job-reserved", delivery_id).unwrap(),
                    b"uncertain",
                )
                .unwrap();
            }
            let latch = latch_path("job-reserved", delivery_id).unwrap();
            let before = std::fs::read(&latch).unwrap();
            assert!(
                report_delivery("job-reserved", delivery_id, "skipped", "old decision").is_err()
            );
            assert_eq!(std::fs::read(latch).unwrap(), before);
            assert!(!outcome_path("job-reserved", delivery_id).unwrap().exists());
        }
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[cfg(unix)]
    #[test]
    fn terminal_journal_repairs_outcome_notice_and_fifo_without_execution() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        let signal = temp.path().join("signal.json");
        std::fs::write(&signal, r#"{"signal":"retained test delivery"}"#).unwrap();
        // Journal deletion requires the local trade-record insert to succeed.
        // Restrict this fixture to that operation; it cannot notify or trade.
        let mock_bin = temp.path().join("bin");
        std::fs::create_dir_all(&mock_bin).unwrap();
        let mock = mock_bin.join("okx-a2a");
        std::fs::write(
            &mock,
            r#"#!/bin/sh
if [ "$#" -eq 5 ] && [ "$1" = trade-records ] && [ "$2" = insert ] && [ "$3" = --input-json ] && [ "$5" = --json ]; then
  printf '%s' "$4" > "$ONCHAINOS_HOME/trade-record-call.json"
  exit 0
fi
exit 1
"#,
        ).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mock, std::fs::Permissions::from_mode(0o700)).unwrap();
        let original_path = std::env::var_os("PATH");
        std::env::set_var("PATH", &mock_bin);
        consent::register_delivery_context(
            "job-journal",
            "7",
            "8",
            Some("session:test"),
            "delivery-1",
            signal.to_str().unwrap(),
            "text",
            1,
        )
        .unwrap();
        super::super::delivery_queue::enqueue("job-journal", "delivery-1").unwrap();
        super::super::delivery_queue::mark_awaiting_decision("job-journal", "delivery-1").unwrap();
        let now = now_secs();
        let outcome = ExecutionOutcome {
            version: OUTCOME_VERSION,
            job_id: "job-journal".to_string(),
            delivery_id: "delivery-1".to_string(),
            venue: String::new(),
            action: String::new(),
            amount: String::new(),
            execution_mode: ExecutionMode::Auto,
            status: OutcomeStatus::FailedBeforeExecution,
            receipt: None,
            reason: Some("test interruption".to_string()),
            failure_category: None,
            notification_pending: true,
            notification_attempts: 0,
            next_notification_attempt_at: 0,
            created_at: now,
            updated_at: now,
        };
        let journal = write_terminal_journal(&outcome).unwrap();
        let result = reconcile_terminal_journals(1, Duration::from_secs(1));
        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        assert_eq!(result.unwrap(), 1);
        assert!(outcome_path("job-journal", "delivery-1").unwrap().exists());
        assert!(notice_ref_path("job-journal", "delivery-1")
            .unwrap()
            .exists());
        assert!(
            !super::super::delivery_queue::contains_delivery("job-journal", "delivery-1").unwrap()
        );
        assert!(!journal.exists());
        let recorded: Value = serde_json::from_slice(
            &std::fs::read(temp.path().join("trade-record-call.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(recorded[0]["deliveryId"], "delivery-1");
        assert_eq!(recorded[0]["status"], "failed_before_execution");
        assert_eq!(recorded[0]["extra"]["signal"], "retained test delivery");
        assert!(signal.exists());
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn durable_outcome_repairs_fifo_even_when_terminal_journal_is_missing() {
        let _guard = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = test_tempdir();
        std::env::set_var("ONCHAINOS_HOME", temp.path());
        for delivery_id in ["delivery-1", "delivery-2"] {
            consent::register_delivery_context(
                "job-outcome-fallback",
                "7",
                "8",
                Some("session:test"),
                delivery_id,
                "/tmp/signal.txt",
                "text",
                1,
            )
            .unwrap();
            super::super::delivery_queue::enqueue("job-outcome-fallback", delivery_id).unwrap();
        }
        super::super::delivery_queue::mark_awaiting_decision("job-outcome-fallback", "delivery-1")
            .unwrap();
        let now = now_secs();
        let outcome = ExecutionOutcome {
            version: OUTCOME_VERSION,
            job_id: "job-outcome-fallback".to_string(),
            delivery_id: "delivery-1".to_string(),
            venue: String::new(),
            action: String::new(),
            amount: String::new(),
            execution_mode: ExecutionMode::Auto,
            status: OutcomeStatus::FailedBeforeExecution,
            receipt: None,
            reason: Some("journal unavailable".to_string()),
            failure_category: None,
            notification_pending: true,
            notification_attempts: 0,
            next_notification_attempt_at: 0,
            created_at: now,
            updated_at: now,
        };
        write_outcome(
            &outcome_path("job-outcome-fallback", "delivery-1").unwrap(),
            &outcome,
        )
        .unwrap();
        assert!(!terminal_journal_path("job-outcome-fallback", "delivery-1")
            .unwrap()
            .exists());
        assert!(
            super::super::delivery_queue::reconcile_terminal_head("job-outcome-fallback").unwrap()
        );
        assert!(!super::super::delivery_queue::contains_delivery(
            "job-outcome-fallback",
            "delivery-1"
        )
        .unwrap());
        assert!(super::super::delivery_queue::contains_delivery(
            "job-outcome-fallback",
            "delivery-2"
        )
        .unwrap());
        std::env::remove_var("ONCHAINOS_HOME");
    }
}
