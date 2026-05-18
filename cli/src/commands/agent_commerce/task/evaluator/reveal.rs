use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_reveal(
    client: &mut TaskApiClient,
    job_id: &str,
    agent_id: &str,
) -> Result<()> {
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    // Pre-check: avoid burning a tx when the reveal window isn't open or the round
    // already settled. Backend returns `{ canReveal: bool, reason?: string }`.
    let can_reveal_path = client.endpoint(job_id, "vote/canReveal");
    let can_resp = client.get_with_identity(&can_reveal_path, &agent_id).await?;
    match can_resp["canReveal"].as_bool() {
        Some(true) => {}
        Some(false) => {
            audit::log(
                "cli",
                "evaluator/vote_reveal_skipped",
                true,
                Duration::default(),
                Some(vec![
                    format!("jobId={job_id}"),
                    format!("agentId={agent_id}"),
                ]),
                None,
            );
            bail!(
                "backend canReveal=false (jobId={job_id}): reveal window not yet open / current round already settled / no commit submitted."
            )
        }
        None => bail!("canReveal response missing boolean field, backend may have returned malformed data: {can_resp}"),
    }

    let reveal_path = client.endpoint(job_id, "vote/reveal");
    // Empty body — backend reads vote+salt from task_dispute_voter.
    let resp = client
        .post_with_identity(&reveal_path, &serde_json::json!({}), &agent_id)
        .await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        job_id,
        signing::extract_biz_type(&resp),
        &agent_id,
    )
    .await?;

    audit::log(
        "cli",
        "evaluator/vote_revealed",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("vote revealed (jobId={job_id})");
    println!("  txHash:       {tx_hash}");
    Ok(())
}
