//! Disabled legacy timeout auto-refund command.
//!
//! Refund V2 must classify every write from fresh authoritative state and bind
//! it to an explicit confirmation. The old command did neither, and a caller
//! could fabricate a local `next-action` event, so no code path in this module
//! performs the mutation.

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// Reject direct use of the legacy write command.
pub async fn handle_claim_auto_refund(client: &mut TaskApiClient, job_id: &str) -> Result<()> {
    let _ = client;
    anyhow::bail!(
        "direct claim-auto-refund is disabled by Refund V2; run `onchainos agent refund-prepare {job_id}`. A cause-specific timeout claim requires a backend V2 contract before this client can safely expose it"
    )
}
