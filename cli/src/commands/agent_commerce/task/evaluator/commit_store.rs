//! Local persistence for commit-reveal evaluator votes.
//!
//! Real backend reveal API (Lark §11348) requires voter to re-send `vote` in the request
//! body. The voter (this CLI) must remember which side they committed so reveal can be called
//! hours/days later without relying on volatile agent session memory.
//!
//! File: `~/.onchainos/evaluator-commits.jsonl` — append-only JSONL, one entry per commit.
//! Lookup is "latest matching `(disputeId, voter)` wins" so retries/new rounds are safe.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

const STORE_FILE: &str = "evaluator-commits.jsonl";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StoredCommit {
    pub dispute_id: String,
    pub side: u8,
    pub voter: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub commit_hash: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tx_hash: String,
    pub committed_at: String,
}

fn store_path() -> Result<PathBuf> {
    let home = crate::home::onchainos_home().context("cannot resolve ~/.onchainos")?;
    if !home.exists() {
        fs::create_dir_all(&home).with_context(|| format!("cannot create {}", home.display()))?;
    }
    Ok(home.join(STORE_FILE))
}

/// Append a commit record. Best-effort: errors are surfaced to caller so they can warn,
/// but the commit itself should already be on-chain before this is called.
pub fn append(entry: &StoredCommit) -> Result<()> {
    let path = store_path()?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("cannot open {}", path.display()))?;
    let line = serde_json::to_string(entry)?;
    writeln!(file, "{line}")?;
    Ok(())
}

/// Find the most recent entry matching `(dispute_id, voter)`. Case-insensitive voter
/// compare since addresses sometimes differ in 0x-prefix casing.
pub fn load_latest(dispute_id: &str, voter: &str) -> Result<Option<StoredCommit>> {
    let path = store_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let file = fs::File::open(&path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut latest: Option<StoredCommit> = None;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }
        let Ok(entry) = serde_json::from_str::<StoredCommit>(&line) else { continue; };
        if entry.dispute_id == dispute_id && entry.voter.eq_ignore_ascii_case(voter) {
            latest = Some(entry);
        }
    }
    Ok(latest)
}

/// Remove all entries matching `(dispute_id, voter)`. Called on TASK_RESOLVED to reclaim
/// space and remove stale records — dispute is terminal, no more reveal possible.
/// Returns the number of entries removed. Idempotent (0 if no match or file missing).
///
/// Implementation: rewrite the file in place (tmp + rename for atomicity). File is small
/// (one line per dispute-vote), so full rewrite is cheap.
pub fn remove(dispute_id: &str, voter: &str) -> Result<usize> {
    let path = store_path()?;
    if !path.exists() {
        return Ok(0);
    }
    let file = fs::File::open(&path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut kept: Vec<String> = Vec::new();
    let mut removed = 0usize;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }
        match serde_json::from_str::<StoredCommit>(&line) {
            Ok(entry) if entry.dispute_id == dispute_id && entry.voter.eq_ignore_ascii_case(voter) => {
                removed += 1;
            }
            _ => kept.push(line),
        }
    }
    if removed == 0 {
        return Ok(0);
    }
    // Atomic replace: write to tmp then rename.
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut f = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&tmp)
            .with_context(|| format!("cannot open {}", tmp.display()))?;
        for line in &kept {
            writeln!(f, "{line}")?;
        }
    }
    fs::rename(&tmp, &path).with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(removed)
}
