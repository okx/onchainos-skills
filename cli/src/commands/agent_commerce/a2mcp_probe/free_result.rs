//! Short-lived, one-time state for a successful free A2MCP invocation.
//!
//! The endpoint has already returned, but its result stays in this local state
//! until the user confirms the single invocation card. Free invocations do not
//! require a wallet, so this state is intentionally independent from prepared
//! payment state.

use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use url::Url;
use uuid::Uuid;

const FREE_RESULT_VERSION: u32 = 1;
const FREE_RESULT_SOURCE: &str = "okx_ai_a2mcp_free_result";
const FREE_RESULT_ID_PREFIX: &str = "a2free_";
const FREE_RESULT_TTL_SECS: u64 = 300;
pub(super) const ERR_FREE_RESULT_EXPIRED_OR_MISSING: &str = "a2mcp_free_result_expired_or_missing";

#[derive(Clone, Debug)]
pub(super) struct FreeResultInput {
    pub service_id: String,
    pub service_name: Option<String>,
    pub provider_agent_id: Option<String>,
    pub endpoint: String,
    pub method: String,
    pub typed_params: Map<String, Value>,
    pub status_code: u16,
    pub result: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FreeResultState {
    version: u32,
    source: String,
    confirmation_id: String,
    created_at: u64,
    expires_at: u64,
    service_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_agent_id: Option<String>,
    endpoint: String,
    method: String,
    typed_params: Map<String, Value>,
    status_code: u16,
    result: Value,
}

impl FreeResultState {
    fn validate(&self, confirmation_id: &str, now: u64) -> Result<()> {
        if self.version != FREE_RESULT_VERSION
            || self.source != FREE_RESULT_SOURCE
            || self.confirmation_id != confirmation_id
            || now >= self.expires_at
        {
            bail!("{ERR_FREE_RESULT_EXPIRED_OR_MISSING}: {confirmation_id}");
        }
        if self.service_id.trim().is_empty() || self.created_at >= self.expires_at {
            bail!("{ERR_FREE_RESULT_EXPIRED_OR_MISSING}: {confirmation_id}");
        }
        let endpoint = Url::parse(&self.endpoint)
            .map_err(|_| anyhow!("{ERR_FREE_RESULT_EXPIRED_OR_MISSING}: {confirmation_id}"))?;
        if endpoint.scheme() != "https" || !matches!(self.method.as_str(), "GET" | "POST") {
            bail!("{ERR_FREE_RESULT_EXPIRED_OR_MISSING}: {confirmation_id}");
        }
        Ok(())
    }

    pub(super) fn confirmation_id(&self) -> &str {
        &self.confirmation_id
    }

    pub(super) fn service_id(&self) -> &str {
        &self.service_id
    }

    pub(super) fn service_name(&self) -> Option<&str> {
        self.service_name.as_deref()
    }

    pub(super) fn provider_agent_id(&self) -> Option<&str> {
        self.provider_agent_id.as_deref()
    }

    pub(super) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(super) fn method(&self) -> &str {
        &self.method
    }

    pub(super) fn typed_params(&self) -> &Map<String, Value> {
        &self.typed_params
    }

    pub(super) fn status_code(&self) -> u16 {
        self.status_code
    }

    pub(super) fn result(&self) -> &Value {
        &self.result
    }
}

fn validate_confirmation_id(confirmation_id: &str) -> Result<()> {
    let suffix = confirmation_id
        .strip_prefix(FREE_RESULT_ID_PREFIX)
        .ok_or_else(|| anyhow!("{ERR_FREE_RESULT_EXPIRED_OR_MISSING}: {confirmation_id}"))?;
    if suffix.len() != 32 || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{ERR_FREE_RESULT_EXPIRED_OR_MISSING}: {confirmation_id}");
    }
    Ok(())
}

fn state_path(confirmation_id: &str) -> Result<PathBuf> {
    validate_confirmation_id(confirmation_id)?;
    let dir = crate::home::onchainos_home()?.join("a2mcp");
    fs::create_dir_all(&dir).context("create A2MCP state directory")?;
    Ok(dir.join(format!("{confirmation_id}.json")))
}

fn read_state(path: &std::path::Path, confirmation_id: &str, now: u64) -> Result<FreeResultState> {
    let bytes = fs::read(path)
        .map_err(|_| anyhow!("{ERR_FREE_RESULT_EXPIRED_OR_MISSING}: {confirmation_id}"))?;
    let state: FreeResultState = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow!("{ERR_FREE_RESULT_EXPIRED_OR_MISSING}: {confirmation_id}"))?;
    state.validate(confirmation_id, now)?;
    Ok(state)
}

pub(super) fn store_free_result(
    input: FreeResultInput,
    created_at: u64,
) -> Result<FreeResultState> {
    let confirmation_id = format!("{FREE_RESULT_ID_PREFIX}{}", Uuid::new_v4().simple());
    let state = FreeResultState {
        version: FREE_RESULT_VERSION,
        source: FREE_RESULT_SOURCE.to_string(),
        confirmation_id: confirmation_id.clone(),
        created_at,
        expires_at: created_at.saturating_add(FREE_RESULT_TTL_SECS),
        service_id: input.service_id,
        service_name: input.service_name,
        provider_agent_id: input.provider_agent_id,
        endpoint: input.endpoint,
        method: input.method,
        typed_params: input.typed_params,
        status_code: input.status_code,
        result: input.result,
    };
    state.validate(&confirmation_id, created_at)?;
    let body = serde_json::to_vec_pretty(&state).context("serialize A2MCP free result state")?;
    crate::home::atomic_write(&state_path(&confirmation_id)?, &body, true)
        .context("write A2MCP free result state")?;
    Ok(state)
}

pub(super) fn load_free_result(confirmation_id: &str, now: u64) -> Result<FreeResultState> {
    let path = state_path(confirmation_id)?;
    match read_state(&path, confirmation_id, now) {
        Ok(state) => Ok(state),
        Err(error) => {
            if error
                .to_string()
                .starts_with(ERR_FREE_RESULT_EXPIRED_OR_MISSING)
            {
                let _ = fs::remove_file(path);
            }
            Err(error)
        }
    }
}

pub(super) fn consume_free_result(confirmation_id: &str, now: u64) -> Result<FreeResultState> {
    let canonical_path = state_path(confirmation_id)?;
    read_state(&canonical_path, confirmation_id, now)?;
    let claim_path = canonical_path.with_file_name(format!(
        ".{confirmation_id}.claim-{}",
        Uuid::new_v4().simple()
    ));
    fs::rename(&canonical_path, &claim_path)
        .map_err(|_| anyhow!("{ERR_FREE_RESULT_EXPIRED_OR_MISSING}: {confirmation_id}"))?;
    let state = match read_state(&claim_path, confirmation_id, now) {
        Ok(state) => state,
        Err(error) => {
            let _ = fs::rename(&claim_path, &canonical_path);
            return Err(error);
        }
    };
    fs::remove_file(&claim_path).context("consume A2MCP free result state")?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home;
    use serde_json::json;

    fn with_home<F: FnOnce()>(sub: &str, f: F) {
        let _lock = home::TEST_ENV_MUTEX.lock().unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test_tmp")
            .join(sub);
        let _ = fs::remove_dir_all(&dir);
        std::env::set_var("ONCHAINOS_HOME", &dir);
        f();
        std::env::remove_var("ONCHAINOS_HOME");
        let _ = fs::remove_dir_all(&dir);
    }

    fn input() -> FreeResultInput {
        FreeResultInput {
            service_id: "service-1".into(),
            service_name: Some("Free service".into()),
            provider_agent_id: Some("8136".into()),
            endpoint: "https://example.com/free".into(),
            method: "POST".into(),
            typed_params: Map::from_iter([("query".into(), json!("BTC"))]),
            status_code: 200,
            result: json!({"answer": 42}),
        }
    }

    #[test]
    fn free_result_is_short_lived_and_consumed_once() {
        with_home("a2mcp_free_result_once", || {
            let stored = store_free_result(input(), 1_000).unwrap();
            let confirmation_id = stored.confirmation_id().to_string();
            assert!(confirmation_id.starts_with("a2free_"));
            assert_eq!(
                load_free_result(&confirmation_id, 1_299).unwrap().result(),
                &json!({"answer":42})
            );

            let consumed = consume_free_result(&confirmation_id, 1_299).unwrap();
            assert_eq!(consumed.status_code(), 200);
            assert!(consume_free_result(&confirmation_id, 1_299).is_err());
        });
    }

    #[test]
    fn expired_free_result_is_removed() {
        with_home("a2mcp_free_result_expired", || {
            let stored = store_free_result(input(), 1_000).unwrap();
            let confirmation_id = stored.confirmation_id().to_string();
            assert!(load_free_result(&confirmation_id, 1_300).is_err());
            assert!(!state_path(&confirmation_id).unwrap().exists());
        });
    }

    #[test]
    fn confirm_command_releases_result_only_with_yes_and_only_once() {
        with_home("a2mcp_free_result_confirm", || {
            let now = crate::commands::payment::session_state::now_unix();
            let stored = store_free_result(input(), now).unwrap();
            let confirmation_id = stored.confirmation_id().to_string();

            let pending = super::super::run_confirm_free(&super::super::ConfirmFreeArgs {
                confirmation_id: confirmation_id.clone(),
                yes: false,
            })
            .unwrap();
            assert_eq!(pending.reason, "free_confirmation_required");
            assert_eq!(pending.payload["providerAgentId"], "8136");
            assert!(pending.payload.get("result").is_none());

            let ready = super::super::run_confirm_free(&super::super::ConfirmFreeArgs {
                confirmation_id: confirmation_id.clone(),
                yes: true,
            })
            .unwrap();
            assert_eq!(ready.phase, "endpoint_result");
            assert_eq!(ready.reason, "free_result");
            assert_eq!(ready.payload["amountDisplay"], "Free");
            assert_eq!(ready.payload["result"], json!({"answer":42}));

            assert!(
                super::super::run_confirm_free(&super::super::ConfirmFreeArgs {
                    confirmation_id,
                    yes: true,
                })
                .is_err()
            );
        });
    }
}
