//! Disabled legacy timeout auto-refund command.
//!
//! Timeout refunds are backend-owned. The old command let a caller fabricate a
//! local `next-action` event and attempt a duplicate buyer-side write, so no code
//! path in this module performs the mutation.

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;

/// Reject direct use of the legacy write command.
pub async fn handle_claim_auto_refund(client: &mut TaskApiClient, job_id: &str) -> Result<()> {
    let _ = client;
    anyhow::bail!(
        "direct claim-auto-refund is disabled by Refund because timeout refunds are backend-owned; use `onchainos agent refund-prepare {job_id}` only to read the authoritative current state. Expired(8) is terminal and confirms any applicable automatic refund"
    )
}
