//! User terms changes (only in the Open state).
//!
//! - `set-max-budget` — change max budget (off-chain; succeeds when the API call returns).
//!
//! Provider changes are handled by `set-asp` in `asp_ops.rs` (off-chain, triggers `job_created`).

use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// set-max-budget — change the max budget (off-chain).
///
/// POST /priapi/v1/aieco/task/{jobId}/setBudget
/// Request:  { "paymentMostTokenAmount": "<human-readable amount>" }
/// Response: { code: 0, data: null }
pub async fn handle_set_max_budget(
    client: &mut TaskApiClient,
    job_id: &str,
    max_budget: &str,
    explicit_agent_id: Option<&str>,
) -> Result<()> {
    let agent_id = match explicit_agent_id {
        Some(id) => id.to_string(),
        None => {
            let (_, _, id) =
                signing::resolve_wallet_and_agent_for_task(client, job_id, None).await?;
            id
        }
    };

    client.post_with_identity(
        &client.endpoint(job_id, "setBudget"),
        &serde_json::json!({
            "paymentMostTokenAmount": max_budget,
        }),
        &agent_id,
    ).await?;

    audit::log(
        "cli",
        "buyer/max_budget_updated",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("maxBudget={max_budget}"),
        ]),
        None,
    );

    println!("✓ Max budget updated to {max_budget}.");
    Ok(())
}
