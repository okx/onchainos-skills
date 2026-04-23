use anyhow::{bail, Result};

use super::commit_store;
use super::helpers::{evaluator_agent_id, parse_job_id};
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;
use crate::commands::Context;

/// E2b: reveal a previously-committed vote. Per real API spec (§11348), voter sends `{ vote }`;
/// backend reads salt from task_dispute_voter and generates revealVote(vote, salt) calldata.
///
/// The `--side` flag is optional:
///   - if provided, used as-is (but warned if it disagrees with the stored record)
///   - if omitted, resolved from `~/.onchainos/evaluator-commits.jsonl` written at commit time
///   - if neither source yields a side, bail with an actionable error
///
/// The side MUST match commit — if it doesn't, the on-chain commitHash won't verify and the
/// contract reverts.
pub async fn run_reveal(dispute_id: String, side_arg: Option<u8>, _ctx: &Context) -> Result<()> {
    let job_id = parse_job_id(&dispute_id)?;
    let (account_id, address) = signing::resolve_wallet(None, None)?;
    let agent_id = evaluator_agent_id();
    let client = TaskApiClient::new();

    // Resolve side: CLI flag > local store > error.
    let stored = commit_store::load_latest(&dispute_id, &address).ok().flatten();
    let side: u8 = match (side_arg, stored.as_ref()) {
        (Some(s), Some(r)) => {
            if s != r.side {
                eprintln!(
                    "warning: --side {s} disagrees with stored commit record (side={}); \
                     using {s} as requested, but reveal will revert on-chain if commit hash mismatches.",
                    r.side
                );
            }
            s
        }
        (Some(s), None) => s,
        (None, Some(r)) => {
            println!("(side auto-resolved from local store: {} for disputeId={})", r.side, dispute_id);
            r.side
        }
        (None, None) => bail!(
            "no --side provided and no local commit record for disputeId={dispute_id} voter={address}. \
             Pass --side 1|2 explicitly, or run commit first so the record is saved."
        ),
    };
    if side != 1 && side != 2 {
        bail!("--side must be 1 (provider wins) or 2 (client wins)");
    }

    // Optional pre-check; mock returns canReveal=true once committed.
    let pre_url = format!("{}?voter={}", client.endpoint(&job_id, "vote/canReveal"), address);
    let pre = client.get_with_identity(&pre_url, &agent_id, &address).await?;
    if pre["data"]["canReveal"] == false {
        let why = pre["data"]["reason"].as_str().unwrap_or("not ready");
        bail!("cannot reveal yet: {why}");
    }

    let resp = client.post_with_identity(
        &client.endpoint(&job_id, "vote/reveal"),
        &serde_json::json!({ "vote": side }),
        &agent_id,
        &address,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        &client, &resp["data"]["uopData"], &account_id, &address,
        signing::BizContext::VoteReveal,
    ).await?;

    let d = &resp["data"];
    println!("vote revealed (disputeId={dispute_id})");
    if let Some(v) = d["revealedVote"].as_u64() {
        let label = if v == 1 { "Provider wins" } else { "Client wins" };
        println!("  revealedVote: {v} ({label})");
    } else {
        let label = if side == 1 { "Provider wins" } else { "Client wins" };
        println!("  revealedVote: {side} ({label})");
    }
    println!("  txHash:       {tx_hash}");
    if d["settled"] == true {
        if let Some(w) = d["winner"].as_str() {
            println!("  settled:      yes ({w} wins)");
        }
        if let Some(v) = d["verdict"].as_str() {
            println!("  verdict:      {v}");
        }
    } else {
        println!("  settled:      no (waiting for other voters)");
    }
    Ok(())
}
