//! Provider applies for a job.
//!
//! Provider action: apply for a job — onchainos agent apply

use anyhow::Result;
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// apply — provider applies for a job
///
/// 1. POST apply API (with identity headers) → fetch uopData
/// 2. Sign uopData + broadcast on-chain
pub async fn handle_apply(
    client: &mut TaskApiClient,
    job_id: &str,
    token_amount: &str,
    token_symbol: &str,
    agent_id: &str,
) -> Result<()> {
    if agent_id.is_empty() {
        anyhow::bail!("--agent-id is required (pass the provider's own agentId; beta backend rejects empty agenticId header)");
    }

    // Guardrail: token_amount must be a positive number. Empty / "0" / non-parseable / non-positive
    // means the agent forgot to read the locked value from [intent:confirm] / [intent:propose] —
    // submitting it would commit on-chain to do the work for free (irreversible). Hard reject early.
    let amt_trim = token_amount.trim();
    let parsed = amt_trim.parse::<f64>();
    if amt_trim.is_empty() || !matches!(parsed, Ok(n) if n > 0.0) {
        anyhow::bail!(
            "--token-amount must be a positive number; got `{token_amount}`. \
             Read the locked `tokenAmount` from the `[intent:confirm]` message you just verified \
             (or fall back to the `[intent:propose]` value if confirm omits it). Empty / 0 / negative \
             = apply for free, irreversible — refusing to broadcast."
        );
    }
    if token_symbol.trim().is_empty() {
        anyhow::bail!(
            "--token-symbol must not be empty; got `{token_symbol}`. \
             Read the locked `tokenSymbol` from the `[intent:confirm]` (or `[intent:propose]`) — do NOT assume USDT."
        );
    }

    let (account_id, address) = signing::resolve_wallet_by_agent_id(agent_id).await?;
    let body = serde_json::json!({
        "tokenAmount": token_amount,
        "tokenSymbol": token_symbol,
    });

    let resp = client.post_with_identity(
        &client.endpoint(job_id, "apply"), &body, agent_id,
    ).await?;

    let tx_hash = signing::sign_uop_and_broadcast(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), agent_id,
        None,
    ).await?;

    audit::log(
        "cli",
        "provider/apply_submitted",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("tokenSymbol={token_symbol}"),
            format!("tokenAmount={token_amount}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("✓ Application submitted (apply), waiting for on-chain confirmation (provider_applied)");
    println!("  Quote: {token_amount} {token_symbol}");
    println!("  txHash: {tx_hash}");
    println!();
    println!("⚠️  Next steps are driven by system notifications — do not proactively message the buyer:");
    println!("    - Do NOT call `xmtp_send` to tell the buyer \"application submitted\" or similar");
    println!("    - You will receive a `provider_applied` system notification after on-chain confirmation");
    println!("    - Once notified, run `onchainos agent next-action --jobid {job_id} --event provider_applied --role provider`,");
    println!("      then follow the output to call `session_status` + `xmtp_send` to send the payment invoice");
    Ok(())
}
