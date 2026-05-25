//! Local cache for the pending-user-decision list (pending-decisions).
//!
//! File: `~/.onchainos/task/pending-decisions.json` (all task-level state lives under `~/.onchainos/task/`).
//!
//! Used together with the sub-agent tool pairing rules for `xmtp_prompt_user` /
//! `[USER_DECISION_RELAY]` to give the user-session agent a **definitive** source of
//! "how many pending decisions are there right now", instead of inferring from chat
//! history scans (which is unreliable and gets clobbered by context truncation).
//!
//! Three subcommands (exposed at the top level as `agent pending-decisions <add|remove|list>`):
//! - `add`: sub agent calls this **before** invoking `xmtp_prompt_user` to register one pending entry.
//! - `remove`: sub agent calls this **before** parsing `[USER_DECISION_RELAY]` to remove one entry.
//! - `list`: user-session agent calls this when entering the "displaying / waiting for user reply" state.
//!
//! Unique key = `(job_id, role, agent_id)` triple:
//! - Same `(job_id, role)` but different `agent_id` (typical: one wallet running multiple provider
//!   agents that are all watching the same public task) → each occupies its own entry, no overwrite.
//! - On duplicate `add`, the old entry with the same triple is replaced, preventing duplicates when
//!   a previous `remove` was missed.

use anyhow::{bail, Result};
use chrono::Utc;
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_TTL_SECS: i64 = 86400;

#[derive(Subcommand)]
pub enum PendingDecisionsCommand {
    /// Register one pending user decision (sub agent calls this **before** invoking xmtp_prompt_user).
    Add {
        /// Full sub-session sessionKey string (obtained by calling the session_status tool first).
        #[arg(long = "sub-key")]
        sub_key: String,
        /// Task jobId.
        #[arg(long = "job-id")]
        job_id: String,
        /// Sub session role: buyer / provider / evaluator.
        #[arg(long)]
        role: String,
        /// The sub session's own agentId (required for multi-agent wallets; third dimension of the unique key).
        #[arg(long = "agent-id")]
        agent_id: String,
        /// One-line summary (used in scenario 1: the "N more pending decisions" brief list at the end of a new prompt).
        #[arg(long)]
        summary: String,
        /// Full original userContent (used in scenario 2: verbatim render in the aggregated detail list when the user asks back).
        #[arg(long = "user-content")]
        user_content: String,
        /// TTL in seconds; default 86400 (24h). Expired entries are auto-cleaned on the next `list` call.
        #[arg(long, default_value_t = DEFAULT_TTL_SECS)]
        ttl: i64,
    },
    /// Remove one pending by (job_id, role, agent_id) (sub agent calls this **before**
    /// parsing [USER_DECISION_RELAY], so the user agent never sees a stale entry).
    Remove {
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        role: String,
        /// The sub session's own agentId (required for multi-agent wallets).
        #[arg(long = "agent-id")]
        agent_id: String,
    },
    /// List current pending entries (auto-cleans expired ones). Optionally filter by --agent-id.
    /// `--format json` emits `{ ok, data: { pending: [...], count } }`;
    /// `--format text` emits a human-readable list, one line each: `<idx>. [Task <short-id> you as <role>(#<agentId>)] <summary>`.
    List {
        #[arg(long, default_value = "json")]
        format: String,
        /// List only the specified agentId's pending entries (optional). Default returns all.
        #[arg(long = "agent-id")]
        agent_id: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PendingEntry {
    pub sub_key: String,
    pub job_id: String,
    pub short_job_id: String,
    pub role: String,
    pub agent_id: String,
    pub summary: String,
    pub user_content: String,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Serialize, Deserialize, Default)]
struct PendingFile {
    pending: Vec<PendingEntry>,
}

fn pending_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("failed to get HOME directory"))?;
    // Aligned with ~/.onchainos/task/<jobId>/ negotiation/arbitration state directories; all task-level state lives under task/.
    let dir = home.join(".onchainos").join("task");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("pending-decisions.json"))
}

fn read_pending() -> Result<PendingFile> {
    let path = pending_path()?;
    if !path.exists() {
        return Ok(PendingFile::default());
    }
    let raw = std::fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(PendingFile::default());
    }
    match serde_json::from_str::<PendingFile>(&raw) {
        Ok(pf) => Ok(pf),
        Err(e) => {
            // Tolerance: back up and reset when the file is corrupted (mirrors wallet_store's tolerance style).
            let backup = path.with_file_name(format!(
                "pending-decisions.broken-{}.json",
                Utc::now().timestamp()
            ));
            let _ = std::fs::copy(&path, &backup);
            eprintln!(
                "[pending] pending-decisions.json parse failed ({e}), backed up to {} and reset",
                backup.display()
            );
            Ok(PendingFile::default())
        }
    }
}

/// Atomic write: write to `.tmp` first, then rename (rename is atomic on POSIX).
fn write_pending_atomic(pf: &PendingFile) -> Result<()> {
    let path = pending_path()?;
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string_pretty(pf)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

fn cleanup_expired(pf: &mut PendingFile) -> usize {
    let now = Utc::now().timestamp();
    let before = pf.pending.len();
    pf.pending.retain(|e| e.expires_at > now);
    before - pf.pending.len()
}

/// Short jobId: first 6 + … + last 4 characters. A 0x... hex value yields `0x1b76…1be1`;
/// a long string ID yields `task-0…long`. Returned as-is if ≤ 12 characters.
pub fn short_job_id(job_id: &str) -> String {
    if job_id.chars().count() <= 12 {
        return job_id.to_string();
    }
    let chars: Vec<char> = job_id.chars().collect();
    let head: String = chars.iter().take(6).collect();
    let tail: String = chars.iter().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{head}…{tail}")
}

fn validate_role(role: &str) -> Result<()> {
    if !["buyer", "provider", "evaluator"].contains(&role) {
        bail!("--role must be buyer / provider / evaluator, got: {role}");
    }
    Ok(())
}

fn role_label(role: &str) -> &'static str {
    match role {
        "buyer" => "buyer",
        "provider" => "seller",
        "evaluator" => "Evaluator Agent",
        _ => "unknown role",
    }
}

pub async fn run(cmd: PendingDecisionsCommand) -> Result<()> {
    match cmd {
        PendingDecisionsCommand::Add {
            sub_key, job_id, role, agent_id, summary, user_content, ttl,
        } => {
            validate_role(&role)?;
            if sub_key.trim().is_empty() {
                bail!("--sub-key cannot be empty");
            }
            if job_id.trim().is_empty() {
                bail!("--job-id cannot be empty");
            }
            if agent_id.trim().is_empty() {
                bail!("--agent-id cannot be empty (third dimension of unique key, required for multi-agent wallets)");
            }
            if ttl <= 0 {
                bail!("--ttl must be a positive number (seconds), got: {ttl}");
            }

            let mut pf = read_pending()?;
            cleanup_expired(&mut pf);

            // Replace any existing entry with the same (job_id, role, agent_id) to avoid duplicates when a previous remove was missed.
            let replaced = pf
                .pending
                .iter()
                .any(|e| e.job_id == job_id && e.role == role && e.agent_id == agent_id);
            pf.pending.retain(|e| {
                !(e.job_id == job_id && e.role == role && e.agent_id == agent_id)
            });

            let now = Utc::now().timestamp();
            let entry = PendingEntry {
                short_job_id: short_job_id(&job_id),
                sub_key,
                job_id,
                role,
                agent_id,
                summary,
                user_content,
                created_at: now,
                expires_at: now + ttl,
            };
            pf.pending.push(entry);
            write_pending_atomic(&pf)?;

            crate::output::success(serde_json::json!({
                "added": true,
                "replaced": replaced,
                "pending_count": pf.pending.len(),
            }));
            Ok(())
        }
        PendingDecisionsCommand::Remove { job_id, role, agent_id } => {
            validate_role(&role)?;
            if agent_id.trim().is_empty() {
                bail!("--agent-id cannot be empty (third dimension of unique key, required for multi-agent wallets)");
            }
            let mut pf = read_pending()?;
            cleanup_expired(&mut pf);
            let before = pf.pending.len();
            pf.pending.retain(|e| {
                !(e.job_id == job_id && e.role == role && e.agent_id == agent_id)
            });
            let removed = before - pf.pending.len();
            write_pending_atomic(&pf)?;
            crate::output::success(serde_json::json!({
                "removed": removed,
                "pending_count": pf.pending.len(),
            }));
            Ok(())
        }
        PendingDecisionsCommand::List { format, agent_id } => {
            let mut pf = read_pending()?;
            let dropped = cleanup_expired(&mut pf);
            // Write back the post-cleanup state so we don't have to redo the cleanup next time.
            if dropped > 0 {
                write_pending_atomic(&pf)?;
            }

            // Filter by agent_id (optional).
            let filtered: Vec<&PendingEntry> = match &agent_id {
                Some(aid) if !aid.is_empty() => {
                    pf.pending.iter().filter(|e| &e.agent_id == aid).collect()
                }
                _ => pf.pending.iter().collect(),
            };

            match format.as_str() {
                "text" => {
                    if filtered.is_empty() {
                        println!("(no pending decisions)");
                    } else {
                        for (i, e) in filtered.iter().enumerate() {
                            println!(
                                "{}. [Task {} you as {}(#{})] {}",
                                i + 1,
                                e.short_job_id,
                                role_label(&e.role),
                                e.agent_id,
                                e.summary
                            );
                        }
                    }
                }
                "json" => {
                    let owned: Vec<PendingEntry> = filtered.into_iter().cloned().collect();
                    let count = owned.len();
                    crate::output::success(serde_json::json!({
                        "pending": owned,
                        "count": count,
                    }));
                }
                other => bail!("--format must be json or text, got: {other}"),
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_job_id_hex_64() {
        assert_eq!(
            short_job_id("0x1b76dabd3bf884626184e3b36b7c65b54929a827a8a26e223c4b8aa868d41be1"),
            "0x1b76…1be1"
        );
    }

    #[test]
    fn short_job_id_passthrough() {
        assert_eq!(short_job_id("0x12"), "0x12");
        assert_eq!(short_job_id("task-1"), "task-1");
        assert_eq!(short_job_id("task-001-12"), "task-001-12");
    }

    #[test]
    fn short_job_id_long_string() {
        assert_eq!(short_job_id("task-001-very-long"), "task-0…long");
    }

    #[test]
    fn validate_role_accepts_canonical() {
        assert!(validate_role("buyer").is_ok());
        assert!(validate_role("provider").is_ok());
        assert!(validate_role("evaluator").is_ok());
        assert!(validate_role("seller").is_err());
        assert!(validate_role("").is_err());
    }

    #[test]
    fn pending_entry_serializes_with_new_fields() {
        let entry = PendingEntry {
            sub_key: "agent:main:xmtp:group:foo".to_string(),
            job_id: "0x3938abcdef".to_string(),
            short_job_id: "0x3938…cdef".to_string(),
            role: "buyer".to_string(),
            agent_id: "100".to_string(),
            summary: "test summary".to_string(),
            user_content: "[Task 0x3938…cdef you as buyer] test content".to_string(),
            created_at: 1700000000,
            expires_at: 1700086400,
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"agent_id\":\"100\""));
        assert!(json.contains("\"user_content\":"));
        let back: PendingEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.agent_id, "100");
        assert_eq!(back.user_content, "[Task 0x3938…cdef you as buyer] test content");
    }
}
