//! Explicit, local execution-mode record for a subscription.
//!
//! A Service Guide + Consent proves that this device has the material required
//! for Guide-driven execution.  It must not, by itself, be treated as the
//! user's durable choice between receive-only and automatic execution.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    SignalOnly,
    GuideDirect,
}

impl ExecutionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SignalOnly => "signal_only",
            Self::GuideDirect => "guide_direct",
        }
    }
}

impl std::str::FromStr for ExecutionMode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim() {
            "signal_only" => Ok(Self::SignalOnly),
            "guide_direct" => Ok(Self::GuideDirect),
            _ => bail!("--execution-mode must be signal_only or guide_direct"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscriptionExecutionConfig {
    version: u32,
    user_agent_id: String,
    service_id: String,
    /// Optional only to support repairing an early/incomplete local record.
    #[serde(default)]
    execution_mode: Option<ExecutionMode>,
    updated_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    Created,
    Repaired,
    Replaced,
}

impl SaveOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Repaired => "repaired",
            Self::Replaced => "replaced",
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn config_path(user_agent_id: &str, service_id: &str) -> Result<PathBuf> {
    if !super::grants::job_id_is_safe(user_agent_id) || !super::grants::job_id_is_safe(service_id) {
        bail!("invalid userAgentId or serviceId")
    }
    Ok(crate::home::onchainos_home()?
        .join("autotrade")
        .join("subscription-config")
        .join(user_agent_id)
        .join(format!("{service_id}.json")))
}

fn load_config(user_agent_id: &str, service_id: &str) -> Result<Option<SubscriptionExecutionConfig>> {
    let path = config_path(user_agent_id, service_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).with_context(|| {
        format!(
            "subscription execution configuration is unreadable: {}",
            path.display()
        )
    })?;
    let config: SubscriptionExecutionConfig = serde_json::from_slice(&bytes)
        .context("subscription execution configuration is invalid")?;
    if config.version != CONFIG_VERSION
        || config.user_agent_id != user_agent_id
        || config.service_id != service_id
    {
        bail!("subscription execution configuration is invalid")
    }
    Ok(Some(config))
}

/// Reads the explicitly selected execution mode. Missing, incomplete, or
/// unreadable records must never be promoted to Guide-driven execution.
pub fn execution_mode(user_agent_id: &str, service_id: &str) -> Result<Option<ExecutionMode>> {
    Ok(load_config(user_agent_id, service_id)?.and_then(|config| config.execution_mode))
}

/// Saves a user-confirmed mode locally. By default this only initializes a
/// missing/incomplete record; callers must use `replace` for an explicit later
/// mode change so orchestration cannot silently overwrite a previous choice.
pub fn save_execution_mode(
    user_agent_id: &str,
    service_id: &str,
    execution_mode: ExecutionMode,
    replace: bool,
) -> Result<SaveOutcome> {
    let existing = load_config(user_agent_id, service_id)?;
    let outcome = match existing.as_ref().and_then(|config| config.execution_mode) {
        None if existing.is_some() => SaveOutcome::Repaired,
        None => SaveOutcome::Created,
        Some(_) if replace => SaveOutcome::Replaced,
        Some(mode) if mode == execution_mode => {
            bail!("subscription executionMode is already {}; use --replace only after a new user confirmation", mode.as_str())
        }
        Some(mode) => {
            bail!("subscription executionMode is already {}; use --replace only after a new user confirmation", mode.as_str())
        }
    };
    let config = SubscriptionExecutionConfig {
        version: CONFIG_VERSION,
        user_agent_id: user_agent_id.to_string(),
        service_id: service_id.to_string(),
        execution_mode: Some(execution_mode),
        updated_at_ms: now_ms(),
    };
    let path = config_path(user_agent_id, service_id)?;
    crate::home::write_secure(&path, &serde_json::to_vec_pretty(&config)?).map_err(|error| {
        anyhow::anyhow!(
            "failed to persist subscription execution configuration at {}: {error}",
            path.display()
        )
    })?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn with_home(test: impl FnOnce()) {
        let _lock = crate::home::TEST_ENV_MUTEX.lock().unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join("subscription_execution_config");
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(&root).unwrap();
        std::env::set_var("ONCHAINOS_HOME", &root);
        test();
        std::env::remove_var("ONCHAINOS_HOME");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn saves_an_explicit_mode_and_does_not_overwrite_it() {
        with_home(|| {
            assert_eq!(
                save_execution_mode("user-1", "service-1", ExecutionMode::SignalOnly, false)
                    .unwrap(),
                SaveOutcome::Created
            );
            assert_eq!(
                execution_mode("user-1", "service-1").unwrap(),
                Some(ExecutionMode::SignalOnly)
            );
            assert!(save_execution_mode("user-1", "service-1", ExecutionMode::GuideDirect, false).is_err());
            assert_eq!(
                save_execution_mode("user-1", "service-1", ExecutionMode::GuideDirect, true)
                    .unwrap(),
                SaveOutcome::Replaced
            );
            assert_eq!(
                execution_mode("user-1", "service-1").unwrap(),
                Some(ExecutionMode::GuideDirect)
            );
        });
    }

    #[test]
    fn repairs_an_incomplete_legacy_record() {
        with_home(|| {
            let path = config_path("user-2", "service-2").unwrap();
            crate::home::write_secure(
                &path,
                br#"{"version":1,"userAgentId":"user-2","serviceId":"service-2","updatedAtMs":1}"#,
            )
            .unwrap();
            assert_eq!(execution_mode("user-2", "service-2").unwrap(), None);
            assert_eq!(
                save_execution_mode("user-2", "service-2", ExecutionMode::SignalOnly, false)
                    .unwrap(),
                SaveOutcome::Repaired
            );
        });
    }
}
