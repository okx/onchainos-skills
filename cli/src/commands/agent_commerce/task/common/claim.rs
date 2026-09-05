//! Account-level reward claim — shared API call layer.
//!
//! The backend `POST /priapi/v1/aieco/task/claim` is an **account-pull** endpoint:
//! it pulls every pending reward for the account across all settled disputes / jobs in one shot,
//! with an empty body, and does not split by jobId / token. The endpoint is role-agnostic
//! (user / asp / evaluator) — any agentId + wallet able to sign can call it.
//!
//! This module only handles: API call + sign + broadcast / JSON parsing + table output.
//! Role-specific wallet/agent resolution (different roles use different `signing::resolve_*`)
//! and role-flavored prompt text (e.g. evaluator's "tell me 'claim rewards'" hint) live in
//! the per-role thin wrappers.
//!
//! Current callers: `evaluator/claim.rs`, `evaluator/claimable.rs`.
//! user / asp integration only needs to resolve the wallet in its own handler and then
//! call the two functions here.

use anyhow::Result;

use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// Call the account-pull claim endpoint, sign uopData, and broadcast. Returns the txHash.
///
/// Callers must first resolve `(account_id, address, agent_id)` themselves (different roles use different resolvers).
pub async fn submit_claim_and_broadcast(
    client: &mut TaskApiClient,
    account_id: &str,
    address: &str,
    agent_id: &str,
) -> Result<String> {
    let path = "/priapi/v1/aieco/task/claim";
    let resp = client
        .post_with_identity(path, &serde_json::json!({}), agent_id)
        .await?;

    signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        account_id,
        address,
        "",
        signing::extract_biz_type(&resp),
        agent_id,
        None,
    )
    .await
}

/// Fetch the account-level claimable list and println-print it. Returns `has_nonzero`, which callers
/// can use to decide whether to surface a role-flavored "claim now" hint.
///
/// The `account` field shown in the header is taken directly from the backend response; empty string when absent.
pub async fn fetch_and_print_claimable(
    client: &mut TaskApiClient,
    agent_id: &str,
) -> Result<bool> {
    let path = "/priapi/v1/aieco/task/claimable";
    let resp = client.get_with_identity(path, agent_id).await?;

    let account = resp["account"].as_str().unwrap_or_default();
    println!("claimable rewards (account={account}, agentId={agent_id})");

    let rewards = resp["rewards"].as_array();
    let mut has_nonzero = false;
    match rewards {
        Some(items) if !items.is_empty() => {
            for r in items {
                let symbol = r["symbol"].as_str().unwrap_or("?");
                let amount = r["amount"].as_str().unwrap_or("0");
                let token = r["tokenAddress"].as_str().unwrap_or("");
                let raw = r["rawAmount"].as_str().unwrap_or("0");
                let nonzero = raw != "0" && !raw.is_empty();
                if nonzero {
                    has_nonzero = true;
                }
                let marker = if nonzero { "•" } else { " " };
                println!("  {marker} {symbol:<8} {amount:>30}  (token={token})");
            }
        }
        _ => {
            println!("  (no rewards)");
        }
    }

    Ok(has_nonzero)
}
