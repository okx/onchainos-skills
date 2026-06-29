//! Negotiation state management.
//!
//! Locally persisted ASP-match list + the current negotiation index;
//! used by the agent when iterating providers.
//!
//! State file: `~/.onchainos/task/{jobId}/negotiate-state.json`.
//! Cleanup: after the user successfully runs `confirm-accept`.

use std::time::Duration;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::audit;
use crate::commands::agent_commerce::task::common::DEBUG_LOG;

/// Recommended provider info (a subset of the `/match` API response).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub provider_address: String,
    pub provider_agent_id: String,
    #[serde(default)]
    pub provider_name: String,
    pub match_score: f64,
    pub credit_score: i64,
    pub capability_summary: String,
    pub completed_task_count: i64,
    /// true = x402 payment mode; false = escrow/direct.
    #[serde(default)]
    pub support_a2mcp: bool,
    #[serde(default)]
    pub services: Vec<ServiceInfo>,
}

/// Service info offered by a provider (returned from `/match`'s `services[]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInfo {
    pub service_id: String,
    pub service_name: String,
    #[serde(default)]
    pub service_description: String,
    /// Service type, e.g. "A2A".
    pub service_type: String,
    /// Service endpoint URL.
    pub endpoint: String,
    #[serde(default)]
    pub sort_order: i64,
    /// Fee amount.
    #[serde(default)]
    pub fee_amount: f64,
    /// Fee token symbol (e.g. "USDT").
    #[serde(default)]
    pub fee_token_symbol: String,
    /// Fee token contract address.
    #[serde(default)]
    pub fee_token: String,
}

/// Negotiation state.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NegotiateState {
    pub job_id: String,
    pub providers: Vec<ProviderInfo>,
    pub current_index: usize,
    pub created_at: String,
    /// Current page (0-based).
    #[serde(default)]
    pub page: usize,
    /// ASP agentIds that failed negotiation (kept across pages; cleared by cleanup on accept success).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_providers: Vec<String>,
}

// ─── Paths ────────────────────────────────────────────────────────────

fn state_dir(job_id: &str) -> Result<std::path::PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("could not resolve HOME directory"))?;
    Ok(home.join(".onchainos").join("task").join(job_id))
}

fn state_path(job_id: &str) -> Result<std::path::PathBuf> {
    Ok(state_dir(job_id)?.join("negotiate-state.json"))
}

// ─── Public functions ────────────────────────────────────────────────────────

/// Save the ASP-match list; index resets to 0.
///
/// `page` is the current page (0-based). `failed_providers` is merged from any prior state.
pub fn save(job_id: &str, providers: Vec<ProviderInfo>, page: usize) -> Result<()> {
    let dir = state_dir(job_id)?;
    std::fs::create_dir_all(&dir)?;

    let existing_failed = load(job_id)
        .map(|s| s.failed_providers)
        .unwrap_or_default();

    let state = NegotiateState {
        job_id: job_id.to_string(),
        providers,
        current_index: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
        page,
        failed_providers: existing_failed,
    };

    let json = serde_json::to_string_pretty(&state)?;
    std::fs::write(state_path(job_id)?, json)?;
    Ok(())
}

/// Load the current state.
pub fn load(job_id: &str) -> Result<NegotiateState> {
    let path = state_path(job_id)?;
    if !path.exists() {
        bail!("Negotiation state not found; run `onchainos agent asp-match --job-id {job_id}` first");
    }
    let raw = std::fs::read_to_string(&path)?;
    let state: NegotiateState = serde_json::from_str(&raw)?;
    Ok(state)
}

/// Return the provider at the current index (do not advance).
pub fn current(job_id: &str) -> Result<Option<ProviderInfo>> {
    let state = load(job_id)?;
    Ok(state.providers.get(state.current_index).cloned())
}

/// Advance to the next provider and return it; returns `None` once the list is exhausted.
pub fn next(job_id: &str) -> Result<Option<ProviderInfo>> {
    let mut state = load(job_id)?;

    state.current_index += 1;

    let json = serde_json::to_string_pretty(&state)?;
    std::fs::write(state_path(job_id)?, json)?;

    Ok(state.providers.get(state.current_index).cloned())
}

/// Save the designated provider (specified via `create-task --provider`; on `job_created` we skip `asp-match`).
pub fn save_designated_provider(job_id: &str, provider_agent_id: &str) -> Result<()> {
    save_designated_provider_with_endpoint(job_id, provider_agent_id, None)
}

pub fn save_designated_provider_with_endpoint(job_id: &str, provider_agent_id: &str, endpoint: Option<&str>) -> Result<()> {
    let dir = state_dir(job_id)?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("designated-provider.json");
    let mut json = serde_json::json!({ "agentId": provider_agent_id });
    if let Some(ep) = endpoint.filter(|s| !s.is_empty()) {
        json["endpoint"] = serde_json::Value::String(ep.to_string());
    }
    std::fs::write(path, serde_json::to_string_pretty(&json)?)?;
    Ok(())
}

/// Check whether the designated-provider file exists (without consuming it).
pub fn has_designated_provider(job_id: &str) -> bool {
    state_dir(job_id)
        .map(|d| d.join("designated-provider.json").exists())
        .unwrap_or(false)
}

/// Read the designated-provider file (read-only; file persists for retries and multi-event scenarios).
/// Cleared explicitly by `cleanup()` (on accept/close) or `clear_designated_provider()` (on mark-failed match).
pub fn get_designated_provider(job_id: &str) -> Result<Option<String>> {
    let path = state_dir(job_id)?.join("designated-provider.json");
    if !path.exists() {
        if cfg!(feature = "debug-log") {
            eprintln!("[designated-provider] file not found: {}", path.display());
        }
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    let result = v["agentId"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string());
    if cfg!(feature = "debug-log") {
        eprintln!("[designated-provider] path={} agentId={:?}", path.display(), result);
    }
    Ok(result)
}

/// Read the persisted endpoint for the designated provider (if saved).
pub fn get_designated_endpoint(job_id: &str) -> Result<Option<String>> {
    let path = state_dir(job_id)?.join("designated-provider.json");
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    Ok(v["endpoint"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string()))
}

/// Remove the designated-provider file (used when mark-failed matches the current designated provider).
pub fn clear_designated_provider(job_id: &str) -> Result<()> {
    let path = state_dir(job_id)?.join("designated-provider.json");
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Mark a provider as failed negotiation (filtered out of subsequent `asp-match` displays).
pub fn mark_failed(job_id: &str, provider_agent_id: &str) -> Result<()> {
    let mut state = match load(job_id) {
        Ok(s) => s,
        Err(_) => {
            let dir = state_dir(job_id)?;
            std::fs::create_dir_all(&dir)?;
            NegotiateState {
                job_id: job_id.to_string(),
                providers: vec![],
                current_index: 0,
                created_at: chrono::Utc::now().to_rfc3339(),
                page: 0,
                failed_providers: vec![],
            }
        }
    };
    let pid = provider_agent_id.to_string();
    if !state.failed_providers.contains(&pid) {
        state.failed_providers.push(pid);
    }
    let json = serde_json::to_string_pretty(&state)?;
    std::fs::write(state_path(job_id)?, json)?;
    audit::log(
        "cli",
        "user/provider_marked_failed",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("provider={provider_agent_id}"),
        ]),
        None,
    );
    println!("✓ Marked provider {provider_agent_id} as failed negotiation (job={job_id})");

    // If the failed provider is the current designated provider, clear the file
    // so that a retry job_created does not re-attempt the same failed provider.
    if let Ok(Some(ref dp)) = get_designated_provider(job_id) {
        if dp == provider_agent_id {
            let _ = clear_designated_provider(job_id);
            if DEBUG_LOG {
                eprintln!("[mark-failed] cleared designated-provider (matched {provider_agent_id})");
            }
        }
    }

    Ok(())
}

/// Load the failed-provider list.
pub fn load_failed(job_id: &str) -> Vec<String> {
    load(job_id)
        .map(|s| s.failed_providers)
        .unwrap_or_default()
}

/// Clean up negotiation state files (called after accept success).
/// Preserves the `attachments/` subdirectory — those files are uploaded
/// during `job_accepted` (Step 1.5) which runs after `confirm-accept`.
pub fn cleanup(job_id: &str) -> Result<()> {
    let dir = state_dir(job_id)?;
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_file() {
            std::fs::remove_file(entry.path())?;
        }
    }
    let attachments_dir = dir.join("attachments");
    if !attachments_dir.exists() {
        let _ = std::fs::remove_dir(&dir);
    }
    Ok(())
}
