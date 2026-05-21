use anyhow::Result;
use std::fs;
use std::time::Duration;

use super::helpers::evidence_dir;
use crate::audit;

/// Placeholder written to disk when the user-customized rubric stripped the
/// §3 verdict template, or when the LLM did not produce output following the
/// template. We still leave jobId / agentId on the page so the audit slot is
/// never empty.
fn placeholder(job_id: &str, agent_id: &str) -> String {
    format!(
        "# Verdict not generated\n\
         \n\
         jobId: {job_id}\n\
         agentId: {agent_id}\n\
         \n\
         vote was committed on-chain, but this round did not produce a verdict per the `references/evaluator-decision-rubric.md` §3 template\n\
         (possible causes: user-customized rubric removed §3, or the evaluator did not follow the template).\n"
    )
}

/// Restore the single-line shell-safe string the LLM passes via `--verdict`
/// back into multi-line markdown: `\n` → newline, `\t` → tab, `\r` → CR,
/// `\\` → `\`, `\"` → `"`.
///
/// Other `\<x>` sequences pass through verbatim (unknown ≠ error; this is
/// forward-compat for LLMs that emit unspecified escapes).
///
/// Design motivation: let the LLM compress the verdict markdown into a
/// single-line `--verdict "..."` argument, bypassing the compat pitfalls of
/// bash heredocs / indentation / cross-platform shells (PowerShell / cmd);
/// at the same time keep the on-disk `verdict.md` in a human-readable
/// multi-line format (mandatory for post-hoc audit / human review).
fn unescape_verdict(raw: &str) -> String {
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

/// Persist the evaluator's verdict markdown to `<evidence_dir>/verdict.md`.
///
/// Called after commit, as a local audit redundancy (the vote is already
/// on-chain; the on-disk copy is only for post-hoc manual review).
/// `verdict = None` → write the placeholder; on failure, flow.rs decides
/// (default: no retry, non-blocking).
pub async fn handle_record(
    job_id: &str,
    agent_id: &str,
    verdict: Option<&str>,
) -> Result<()> {
    let dir = evidence_dir(job_id, agent_id)?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("verdict.md");

    let (is_placeholder, content) = match verdict {
        Some(v) if !v.is_empty() => (false, unescape_verdict(v)),
        _ => (true, placeholder(job_id, agent_id)),
    };
    fs::write(&path, &content)?;

    let event = if is_placeholder {
        "evaluator/verdict_placeholder_written"
    } else {
        "evaluator/verdict_written"
    };
    audit::log(
        "cli",
        event,
        true,
        Duration::default(),
        Some(vec![
            format!("jobId={job_id}"),
            format!("agentId={agent_id}"),
            format!("path={}", path.display()),
        ]),
        None,
    );

    println!("verdict written (jobId={job_id})");
    println!("  path: {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::unescape_verdict;

    #[test]
    fn unescape_newline_tab_cr() {
        assert_eq!(unescape_verdict("line1\\nline2"), "line1\nline2");
        assert_eq!(unescape_verdict("col1\\tcol2"), "col1\tcol2");
        assert_eq!(unescape_verdict("dos\\r\\nstyle"), "dos\r\nstyle");
    }

    #[test]
    fn unescape_backslash_and_quote() {
        // Escaped backslash: `\\` (two source chars) → `\` (one output char).
        assert_eq!(unescape_verdict("path\\\\to\\\\file"), "path\\to\\file");
        // Escaped quote: `\"` (two source chars) → `"` (one output char).
        assert_eq!(unescape_verdict("He said \\\"hi\\\""), "He said \"hi\"");
    }

    #[test]
    fn unknown_escape_passes_through() {
        // `\q` is not a recognized escape; keep both chars verbatim.
        assert_eq!(unescape_verdict("foo\\qbar"), "foo\\qbar");
        // Trailing lone backslash also survives.
        assert_eq!(unescape_verdict("foo\\"), "foo\\");
    }

    #[test]
    fn realistic_verdict_roundtrip() {
        // Single-line input as the LLM would write it on the command line.
        let raw = "Verdict\\n\\nJob ID: 0xabc\\nvote: 1\\nReasoning: per #3, client submitted no evidence.";
        let out = unescape_verdict(raw);
        assert!(out.starts_with("Verdict\n\n"));
        assert!(out.contains("\nvote: 1\n"));
        assert!(out.ends_with("no evidence."));
        // Ensure no stray literal `\n` survived.
        assert!(!out.contains("\\n"));
    }
}