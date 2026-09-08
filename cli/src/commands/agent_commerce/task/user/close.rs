//! Disabled legacy close command.
//!
//! Closing a funded V2 task is a refund-related funds mutation. It must use
//! Refund preparation, confirmation, and reconciliation instead of this
//! context-free legacy entry point.

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// Reject direct use and preserve deterministic migration guidance.
pub async fn handle_close(
    client: &mut TaskApiClient,
    job_id: &str,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    let _ = (client, explicit_agent_id);
    anyhow::bail!(
        "direct close is disabled for V2 tasks; run `onchainos agent refund-prepare {job_id}` and execute only the returned confirmed action"
    )
}
