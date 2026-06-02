use anyhow::{bail, Result};
use std::time::Duration;

use crate::audit;
use crate::commands::agent_commerce::task::common::network::task_api_client::TaskApiClient;
use crate::commands::agent_commerce::task::signing;

/// Restore the single-line shell-safe `--reason` argument back into multi-line
/// markdown: `\n` → newline, `\t` → tab, `\r` → CR, `\\` → `\`, `\"` → `"`.
/// Unknown `\<x>` sequences pass through verbatim (forward-compat).
///
/// Design motivation: let the LLM compress the verdict into a single-line
/// `--reason "..."` argument, bypassing bash heredoc / cross-platform shell
/// pitfalls; backend receives properly-newlined markdown for audit display.
fn unescape_reason(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

pub async fn handle_commit(
    client: &mut TaskApiClient,
    job_id: &str,
    vote: u8,
    reason: &str,
    reason_summary: &str,
    agent_id: &str,
) -> Result<()> {
    if vote != 0 && vote != 1 {
        bail!("--vote must be 0 (Approve, Client wins) or 1 (Reject, Provider wins)");
    }
    let reason = unescape_reason(reason.trim());
    if reason.trim().is_empty() {
        bail!("--reason must not be empty");
    }
    let reason_summary = reason_summary.trim().to_string();
    if reason_summary.is_empty() {
        bail!("--reason-summary must not be empty");
    }
    let summary_len = reason_summary.chars().count();
    if summary_len > 30 {
        bail!("--reason-summary must be ≤30 characters (got {summary_len}); compress the verdict further");
    }
    let (account_id, address, agent_id) =
        signing::resolve_wallet_and_agent_for_evaluator(agent_id).await?;

    let body = serde_json::json!({ "vote": vote });
    let path = client.endpoint(job_id, "vote/commit");
    let resp = client.post_with_identity(
        &path,
        &body,
        &agent_id,
    ).await?;

    // Backend commit response returns `salt` and `commitHash`; broadcast bizContext.
    let salt = resp["salt"].as_str()
        .unwrap_or("");
    if salt.is_empty() {
        bail!("backend did not return salt, cannot broadcast vote/commit");
    }
    let commit_hash = resp["commitHash"].as_str().unwrap_or("");

    let tx_hash = signing::sign_uop_and_broadcast_with_commit_meta(
        client, &resp["uopData"], &account_id, &address,
        job_id, signing::extract_biz_type(&resp), &agent_id,
        salt, vote, &reason, &reason_summary,
    ).await?;

    let vote_label = if vote == 0 { "Approve (Client wins)" } else { "Reject (Provider wins)" };

    audit::log(
        "cli",
        "evaluator/vote_committed",
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("vote={vote}"),
            format!("reasonLen={}", reason.chars().count()),
            format!("reasonSummaryLen={summary_len}"),
            format!("commitHash={commit_hash}"),
            format!("txHash={tx_hash}"),
        ]),
        None,
    );

    println!("vote committed (jobId={job_id})");
    println!("  vote:       {vote} ({vote_label})");
    println!("  voter:      {address}");
    if !commit_hash.is_empty() {
        println!("  commitHash: {commit_hash}");
    }
    println!("  txHash:     {tx_hash}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::unescape_reason;

    #[test]
    fn unescape_newline_tab_cr() {
        assert_eq!(unescape_reason("line1\\nline2"), "line1\nline2");
        assert_eq!(unescape_reason("col1\\tcol2"), "col1\tcol2");
        assert_eq!(unescape_reason("dos\\r\\nstyle"), "dos\r\nstyle");
    }

    #[test]
    fn unescape_backslash_and_quote() {
        assert_eq!(unescape_reason("path\\\\to\\\\file"), "path\\to\\file");
        assert_eq!(unescape_reason("He said \\\"hi\\\""), "He said \"hi\"");
    }

    #[test]
    fn unknown_escape_passes_through() {
        assert_eq!(unescape_reason("foo\\qbar"), "foo\\qbar");
        assert_eq!(unescape_reason("foo\\"), "foo\\");
    }

    #[test]
    fn realistic_verdict_roundtrip() {
        let raw = "Verdict\\n\\nJob ID: 0xabc\\nvote: 1\\nReasoning: per #3, client submitted no evidence.";
        let out = unescape_reason(raw);
        assert!(out.starts_with("Verdict\n\n"));
        assert!(out.contains("\nvote: 1\n"));
        assert!(out.ends_with("no evidence."));
        assert!(!out.contains("\\n"));
    }
}
