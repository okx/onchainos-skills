//! 仲裁者 reveal 投票（commit-reveal 第二阶段）— onchainos agent evaluator reveal <disputeId>
//!
//! Driven by the `reveal_started` system event whose envelope carries the `disputeId` to
//! reveal. Backend reads vote+salt from `task_dispute_voter` keyed by (disputeId, voter),
//! so the request body is empty and the CLI takes only the disputeId argument — no
//! `--side`, no local store.

use anyhow::{bail, Result};

use super::helpers::parse_job_id;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

pub async fn handle_reveal(
    client: &mut TaskApiClient,
    dispute_id: &str,
    agent_id_hint: Option<&str>,
) -> Result<()> {
    let job_id = parse_job_id(dispute_id)?;
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id_hint).await?;

    // Pre-check: avoid burning a tx when the reveal window isn't open or the round
    // already settled. Backend returns `{ canReveal: bool, reason?: string }`.
    let can_reveal_path = client.endpoint(&job_id, "vote/canReveal");
    let can_resp = client.get_with_identity(&can_reveal_path, &agent_id).await?;
    match can_resp["canReveal"].as_bool() {
        Some(true) => {}
        Some(false) => bail!(
            "后端 canReveal=false（disputeId={dispute_id}）：reveal 窗口尚未开启 / 本轮已结算 / 未 commit。\n\
             收到 `reveal_started` 事件后重试；若本轮已结算，改跑 `evaluator claim`（account 级 pull 所有奖励）。"
        ),
        None => bail!("canReveal 响应缺少布尔字段，后端可能返回异常: {can_resp}"),
    }

    let reveal_path = client.endpoint(&job_id, "vote/reveal");
    // Empty body — backend reads vote+salt from task_dispute_voter.
    let resp = client
        .post_with_identity(&reveal_path, &serde_json::json!({}), &agent_id)
        .await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client,
        &resp["uopData"],
        &account_id,
        &address,
        &job_id,
        signing::BizContext::VoteReveal,
        &agent_id,
    )
    .await?;

    println!("vote revealed (disputeId={dispute_id})");
    if let Some(v) = resp["revealedVote"].as_u64() {
        let label = if v == 1 { "Provider wins" } else { "Client wins" };
        println!("  revealedVote: {v} ({label})");
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
