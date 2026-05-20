use anyhow::Result;
use std::fs;
use std::time::Duration;

use super::helpers::evidence_dir;
use crate::audit;

/// 用户自定义 rubric 删除了 §3 裁决书模板、或 LLM 未按模板产出时落盘的占位符。
/// 留下 jobId / agentId 让审计槽位永不为空。
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

/// 把 LLM 通过 `--verdict` 传入的单行 shell-safe 字符串还原为多行 markdown：
/// `\n` → newline, `\t` → tab, `\r` → CR, `\\` → `\`, `\"` → `"`。
///
/// 其他 `\<x>` 序列原样保留（不识别 ≠ 报错；前向兼容 LLM 写出未约定的转义）。
///
/// 设计动机：让 LLM 把 verdict markdown 压成单行 `--verdict "..."` 参数，绕开
/// bash heredoc / 缩进 / 跨平台 shell（PowerShell / cmd）的兼容性陷阱；同时让
/// 落盘的 `verdict.md` 保持人类可读的多行格式（事后审计 / 人工复议必需）。
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

/// 把 evaluator 产出的裁决书 markdown 落盘到 `<evidence_dir>/verdict.md`。
///
/// commit 后调用，作为本地审计冗余（vote 已上链，落盘仅供事后人工/复议核对）。
/// `verdict` 为 None → 写入占位符；失败由 flow.rs 决定如何处理（默认不重试、不阻塞）。
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