//! 仲裁者 reveal 投票（commit-reveal 第二阶段）— onchainos agent evaluator reveal

use anyhow::{bail, Result};

use super::commit_store;
use super::helpers::parse_job_id;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

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
pub async fn handle_reveal(
    client: &mut TaskApiClient,
    dispute_id: &str,
    side_arg: Option<u8>,
) -> Result<()> {
    let job_id = parse_job_id(dispute_id)?;
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator().await?;

    // Resolve side: CLI flag > local store > error.
    let stored = commit_store::load_latest(dispute_id, &address).ok().flatten();
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

    // Pre-check: GET /vote/canReveal — avoid burning a tx when the window isn't open yet
    // or the round already settled. Returns `{ canReveal: bool }`; false → actionable bail.
    let can_reveal_path = client.endpoint(&job_id, "vote/canReveal");
    let can_resp = client
        .get_with_identity(&can_reveal_path, &agent_id)
        .await?;
    match can_resp["canReveal"].as_bool() {
        Some(true) => {}
        Some(false) => bail!(
            "后端 canReveal=false（disputeId={dispute_id}）：reveal 窗口尚未开启 / 本轮已结算 / 未 commit。\n\
             收到 `reveal_started` 事件后重试；若本轮已结算，改跑 `evaluator claim <jobId>`。"
        ),
        None => bail!("canReveal 响应缺少布尔字段，后端可能返回异常: {can_resp}"),
    }

    let reveal_path = client.endpoint(&job_id, "vote/reveal");
    let resp = client.post_with_identity(
        &reveal_path,
        &serde_json::json!({ "vote": side }),
        &agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        &job_id, signing::BizContext::VoteReveal,
    ).await?;

    println!("vote revealed (disputeId={dispute_id})");
    if let Some(v) = resp["revealedVote"].as_u64() {
        let label = if v == 1 { "Provider wins" } else { "Client wins" };
        println!("  revealedVote: {v} ({label})");
    } else {
        let label = if side == 1 { "Provider wins" } else { "Client wins" };
        println!("  revealedVote: {side} ({label})");
    }
    println!("  txHash:       {tx_hash}");
    if resp["settled"] == true {
        if let Some(w) = resp["winner"].as_str() {
            println!("  settled:      yes ({w} wins)");
        }
        if let Some(v) = resp["verdict"].as_str() {
            println!("  verdict:      {v}");
        }
    } else {
        println!("  settled:      no (waiting for other voters)");
    }
    Ok(())
}
