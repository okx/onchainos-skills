//! Pending-decisions v2 — redesigned queue with single-active invariant,
//! implicit state machine, (jobId, role, agentId, toAgentId?) primary key,
//! and LLM-playbook output.
//!
//! Files (all under `~/.onchainos/task/`, separate from v1):
//! - `pending-decisions-new.json` — queue data
//! - `pending-decisions-new.lock` — fs2 flock file
//! - `last-display.json` — snapshot for index → sub_key mapping
//!
//! Four subcommands (`agent pending-decisions-v2 <request|resolve|pick|list>`):
//! - `request`: sub adds a decision; overwrites if same sub_key already exists.
//! - `resolve`: user-session relays user's reply to the active decision.
//! - `pick`: user-session promotes selected entry from list to active.
//! - `list`: query current queue (markdown / json), refreshes snapshot.

use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use clap::{Subcommand, ValueEnum};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::NamedTempFile;

const DEFAULT_TTL_DAYS: u64 = 7;
const TTL_ENV_VAR: &str = "ONCHAINOS_PENDING_DECISIONS_TTL_DAYS";
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// `defer` keyword whitelist — user-session uses these to skip relay and just end the turn.
/// CLI doesn't actually consume these (user-session matches on its own), but documented here
/// so the LLM playbook can reference a consistent list.
pub const DEFER_KEYWORDS: &[&str] = &[
    // Chinese
    "等会儿", "等等", "等一下", "稍后", "晚点", "先放着", "先不管", "回头再看",
    // English
    "skip", "later", "wait", "hold on", "not now", "defer",
];

// ─── Data model ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Status {
    Active,
    Queued,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct PendingEntry {
    job_id: String,
    role: String,
    agent_id: String,
    /// Peer agent id for relay (task sub session). `None` for backup sessions
    /// with no peer yet — relay drops `--to-agent-id` and lands on
    /// `backup:<jobId>`. Set explicitly by the caller at `request` time;
    /// `serde(default)` keeps backward-compat with on-disk JSON written before
    /// this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    to_agent_id: Option<String>,
    user_content: String,
    list_label: String,
    /// Optional sub-provided llmContent. If set, the `request` push playbook
    /// uses this string verbatim instead of CLI's default v2 template.
    /// Sub controls the user-facing instruction body (option descriptions,
    /// routing hints, etc.) but should still end with "call pending-decisions-v2
    /// resolve --user-reply ..." so the queue lifecycle is managed by CLI.
    /// `serde(default)` keeps backward-compat with existing on-disk JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    llm_content_override: Option<String>,
    /// Originating chain event for this decision (e.g. `job_submitted` /
    /// `job_rejected` / `job_disputed` / `submit_deadline_warn`). At resolve
    /// time the CLI emits a system-shaped relay envelope with
    /// `event = "user_decision_<source_event>"`, so the receiving sub session
    /// can dispatch to its existing `next-action --event user_decision_<X>`
    /// handler — no string-prefix parsing, no keyword-mapping in the sub.
    ///
    /// Optional for backward compatibility: if absent at resolve time, the CLI
    /// falls back to a generic `user_decision` event (still system-shaped,
    /// sub handles via a default branch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_event: Option<String>,
    status: Status,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
struct Queue {
    entries: Vec<PendingEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct DisplayItem {
    index: usize,
    job_id: String,
    role: String,
    agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    to_agent_id: Option<String>,
    list_label: String,
}

/// Primary-key match for `PendingEntry`. Same `(jobId, role, agent_id, to_agent_id?)`
/// → same entry (overwrite); different on any field → different entry.
fn entry_matches(
    e: &PendingEntry,
    job_id: &str,
    role: &str,
    agent_id: &str,
    to_agent_id: Option<&str>,
) -> bool {
    e.job_id == job_id
        && e.role == role
        && e.agent_id == agent_id
        && e.to_agent_id.as_deref() == to_agent_id
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
struct DisplaySnapshot {
    displayed_at: Option<DateTime<Utc>>,
    items: Vec<DisplayItem>,
}

// ─── Paths ──────────────────────────────────────────────────────────────

fn task_dir() -> Result<PathBuf> {
    // Respect ONCHAINOS_HOME (project-local override per CLAUDE.md); fall back to ~/.onchainos.
    let base = match std::env::var("ONCHAINOS_HOME") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => {
            let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("unable to determine HOME directory"))?;
            home.join(".onchainos")
        }
    };
    let dir = base.join("task");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn queue_path() -> Result<PathBuf> {
    Ok(task_dir()?.join("pending-decisions-new.json"))
}

fn lock_path() -> Result<PathBuf> {
    Ok(task_dir()?.join("pending-decisions-new.lock"))
}

fn snapshot_path() -> Result<PathBuf> {
    Ok(task_dir()?.join("last-display.json"))
}

// ─── TTL ────────────────────────────────────────────────────────────────

fn load_global_ttl() -> Duration {
    let days = std::env::var(TTL_ENV_VAR)
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TTL_DAYS);
    Duration::from_secs(days * 24 * 60 * 60)
}

// ─── Lock + atomic IO ──────────────────────────────────────────────────

/// Acquire exclusive flock with a 5-second timeout.
fn acquire_lock() -> Result<std::fs::File> {
    let path = lock_path()?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    let deadline = std::time::Instant::now() + LOCK_TIMEOUT;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(file),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() > deadline {
                    bail!("pending-decisions lock timed out after {:?}", LOCK_TIMEOUT);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => bail!("acquire flock failed: {e}"),
        }
    }
}

/// Append a timestamped line to /tmp/onchainos-cli-mode.log for verifying the
/// CLI-mode bypass branches in handle_request / handle_resolve. Best-effort;
/// any IO error is swallowed so trace failures never break the main flow.
fn trace_log(line: &str) {
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/onchainos-cli-mode.log")
        .and_then(|mut f| writeln!(f, "[{}] {}", Utc::now().to_rfc3339(), line));
}

fn read_queue() -> Result<Queue> {
    let path = queue_path()?;
    if !path.exists() {
        return Ok(Queue::default());
    }
    let raw = std::fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(Queue::default());
    }
    Ok(serde_json::from_str::<Queue>(&raw).unwrap_or_default())
}

/// P1-B idempotency helper for `next-action`: returns `true` when the queue
/// already contains a pending decision entry for the given (job_id, role)
/// pair. Used to short-circuit duplicate chain events (e.g. job_created
/// firing into both task sub + backup sub) without forcing the LLM to run
/// `pending-decisions-v2 list --format json` as a separate turn.
///
/// Best-effort: read-only, no lock; on read failure returns `false` so the
/// caller falls back to the normal event flow.
pub fn has_pending_for_job(job_id: &str, role: &str) -> bool {
    let queue = match read_queue() {
        Ok(q) => q,
        Err(e) => {
            trace_log(&format!(
                "has_pending_for_job read_queue failed: {e}; returning false"
            ));
            return false;
        }
    };
    queue
        .entries
        .iter()
        .any(|e| e.job_id == job_id && e.role == role)
}

/// Cancel all pending decision entries that match the given `job_id`.
/// Returns the number of entries removed. Used by `session-cleanup` to
/// batch-clear stale pending decisions on terminal state without requiring
/// the LLM to know individual sub_keys.
pub fn cancel_all_for_job(job_id: &str) -> Result<usize> {
    let _lock = acquire_lock()?;
    let mut q = read_queue()?;
    ensure_invariant_and_evict(&mut q);

    let before = q.entries.len();
    q.entries.retain(|e| e.job_id != job_id);
    let removed = before - q.entries.len();

    if removed > 0 {
        let snap = build_snapshot(&q);
        write_snapshot_atomic(&snap)?;
        write_queue_atomic(&q)?;
    }
    Ok(removed)
}

fn write_queue_atomic(queue: &Queue) -> Result<()> {
    let path = queue_path()?;
    let dir = path.parent().ok_or_else(|| anyhow::anyhow!("no parent dir"))?;
    let mut tmp = NamedTempFile::new_in(dir)?;
    let serialized = serde_json::to_string_pretty(queue)?;
    tmp.write_all(serialized.as_bytes())?;
    tmp.flush()?;
    tmp.persist(&path)
        .map_err(|e| anyhow::anyhow!("persist queue file failed: {e}"))?;
    Ok(())
}

fn read_snapshot() -> DisplaySnapshot {
    let path = match snapshot_path() {
        Ok(p) => p,
        Err(_) => return DisplaySnapshot::default(),
    };
    if !path.exists() {
        return DisplaySnapshot::default();
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return DisplaySnapshot::default(),
    };
    serde_json::from_str::<DisplaySnapshot>(&raw).unwrap_or_default()
}

fn write_snapshot_atomic(snap: &DisplaySnapshot) -> Result<()> {
    let path = snapshot_path()?;
    let dir = path.parent().ok_or_else(|| anyhow::anyhow!("no parent dir"))?;
    let mut tmp = NamedTempFile::new_in(dir)?;
    let serialized = serde_json::to_string_pretty(snap)?;
    tmp.write_all(serialized.as_bytes())?;
    tmp.flush()?;
    tmp.persist(&path)
        .map_err(|e| anyhow::anyhow!("persist snapshot file failed: {e}"))?;
    Ok(())
}

// ─── Invariant + TTL eviction ──────────────────────────────────────────

/// Self-heal invariants + evict expired entries. Called inside every locked op.
fn ensure_invariant_and_evict(queue: &mut Queue) -> usize {
    let now = Utc::now();
    let ttl = load_global_ttl();
    let pre_len = queue.entries.len();

    // 1. Multi-active heal: keep oldest active, demote others to queued
    let actives: Vec<usize> = queue
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.status == Status::Active)
        .map(|(i, _)| i)
        .collect();
    if actives.len() > 1 {
        let mut sorted = actives;
        sorted.sort_by_key(|&i| queue.entries[i].created_at);
        for &i in &sorted[1..] {
            queue.entries[i].status = Status::Queued;
        }
    }

    // 2. Global TTL eviction
    queue.entries.retain(|e| {
        let age = (now - e.created_at).num_seconds().max(0) as u64;
        age < ttl.as_secs()
    });
    let evicted = pre_len - queue.entries.len();

    // 3a. Normalize entry order: Active first (always pinned to index 0 because the user is
    //     "currently working on" it), then Queued entries in LIFO by created_at (newest first).
    //     Invariant guarantees at most one Active, so the Active-vs-Active branch is unreachable.
    //     This ordering drives both the queue file and the display snapshot, so `pick --index 1`
    //     always refers to the active entry (no-op promotion) and `pick --index 2+` always refers
    //     to a queued entry — keeping the "switch N" UX (jump to the Nth remaining item) cleanly
    //     mappable to `pick --index (N+1)`.
    queue.entries.sort_by(|a, b| {
        use std::cmp::Ordering;
        match (a.status == Status::Active, b.status == Status::Active) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => b.created_at.cmp(&a.created_at),
        }
    });

    // 3b. If eviction killed the active entry, promote the newest queued to recover.
    //    NOTE: only triggers when `evicted > 0`. Otherwise "no active + N queued" is a
    //    valid state (selection mode after resolve with queue >= 2) and must be preserved.
    if evicted > 0 {
        let has_active = queue.entries.iter().any(|e| e.status == Status::Active);
        if !has_active {
            if let Some(newest) = queue
                .entries
                .iter_mut()
                .filter(|e| e.status == Status::Queued)
                .max_by_key(|e| e.created_at)
            {
                newest.status = Status::Active;
            }
        }
    }

    evicted
}

// ─── CLI ────────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum PendingDecisionsV2Command {
    /// (sub) Enqueue a new user-decision request. Overwrites the entry with
    /// the same `(jobId, role, agentId, toAgentId?)` key.
    Request {
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        role: String,
        #[arg(long = "agent-id")]
        agent_id: String,
        /// Peer agent id (task sub session). Omit for backup sessions with no
        /// peer — relay then targets `backup:<jobId>`.
        #[arg(long = "to-agent-id")]
        to_agent_id: Option<String>,
        /// Full user-facing text (verbatim rendered to chat).
        #[arg(long = "user-content", required_unless_present = "user_content_file")]
        user_content: Option<String>,
        /// Path to a file whose content is used as user-facing text.
        /// Mutually exclusive with `--user-content`. CLI reads the file
        /// internally so the caller never needs to hold the content.
        #[arg(long = "user-content-file", conflicts_with = "user_content")]
        user_content_file: Option<String>,
        /// Short one-line label for the multi-decision list view.
        #[arg(long = "list-label")]
        list_label: String,
        /// (Optional, v1-compat) Custom llmContent string. If set, CLI uses this
        /// verbatim as the push playbook's llmContent instead of the v2 default
        /// template. Sub should still end the string with an instruction to call
        /// `pending-decisions-v2 resolve --user-reply "<verbatim>"` so queue
        /// lifecycle stays managed by CLI.
        #[arg(long = "llm-content")]
        llm_content: Option<String>,
        /// Originating chain event for this decision (e.g. `job_submitted` /
        /// `job_rejected` / `job_disputed` / `submit_deadline_warn`). At resolve
        /// time the CLI emits a system-shaped relay envelope with
        /// `event = "user_decision_<source_event>"`. Sub then routes via its
        /// existing `next-action --event user_decision_<X>` handler.
        #[arg(long = "source-event")]
        source_event: Option<String>,
    },

    /// (sub, synchronous direct push — bypass queue + playbook emission)
    /// Same routing arguments as `Request`, but immediately invokes
    /// `okx-a2a user decision-request` from inside the CLI and returns. The
    /// caller never sees a playbook to execute — push is already done when
    /// this command exits. Use when the sub agent has all the inputs ready
    /// and just wants the card delivered without the LLM having to re-run
    /// any tool.
    #[command(name = "request-prompt")]
    RequestPrompt {
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        role: String,
        #[arg(long = "agent-id")]
        agent_id: String,
        #[arg(long = "to-agent-id")]
        to_agent_id: Option<String>,
        #[arg(long = "user-content", required_unless_present = "user_content_file")]
        user_content: Option<String>,
        #[arg(long = "user-content-file", conflicts_with = "user_content")]
        user_content_file: Option<String>,
        #[arg(long = "list-label")]
        list_label: String,
        #[arg(long = "llm-content")]
        llm_content: Option<String>,
        #[arg(long = "source-event")]
        source_event: Option<String>,
    },

    /// (user-session) Resolve the current active decision with user's reply.
    Resolve {
        #[arg(long = "user-reply")]
        user_reply: String,
    },

    /// (user-session, CLI-driver bypass) Resolve a decision without consulting
    /// the queue file — caller passes every routing field explicitly so the
    /// envelope can be built and dispatched. Pairs with `request`'s
    /// OKX_A2A_IS_CLI=1 bypass; used when a CLI driver loop (Claude Code / Codex)
    /// owns turn-taking and never persists queue state to disk.
    #[command(name = "resolve-with-sessionkey")]
    ResolveWithSessionkey {
        #[arg(long = "user-reply")]
        user_reply: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        role: String,
        #[arg(long = "agent-id")]
        agent_id: String,
        /// Peer agent id (task sub). Omit for backup sessions.
        #[arg(long = "to-agent-id")]
        to_agent_id: Option<String>,
        #[arg(long = "source-event")]
        source_event: String,
    },

    /// (user-session, queue-backed variant of resolve-with-sessionkey) Same envelope
    /// construction as `resolve-with-sessionkey`, but also removes the matching entry
    /// from the persisted queue and emits a playbook keyed to `resolve-prompt` so
    /// the LLM doesn't accidentally retry against another resolver. Pairs with
    /// `playbook_push_prompt_user` (the queue-mode push variant), so a queue-mode
    /// push → queue-mode relay round-trip stays consistent.
    #[command(name = "resolve-prompt")]
    ResolvePrompt {
        #[arg(long = "user-reply")]
        user_reply: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        role: String,
        #[arg(long = "agent-id")]
        agent_id: String,
        /// Peer agent id (task sub). Omit for backup sessions.
        #[arg(long = "to-agent-id")]
        to_agent_id: Option<String>,
        #[arg(long = "source-event")]
        source_event: String,
    },

    /// (user-session) Pick entry by 1-based index from the displayed list.
    Pick {
        #[arg(long)]
        index: usize,
    },

    /// Query the current queue. Refreshes the display snapshot as a side effect.
    List {
        #[arg(long, default_value = "markdown")]
        format: ListFormat,
    },

    /// (user-session) Silently cancel a pending decision (the sub is NOT notified;
    /// it will eventually TTL-evict or be retriggered by a new system event).
    /// If the cancelled entry was Active, the newest Queued entry is auto-promoted (LIFO).
    Cancel {
        /// Cancel by 1-based index from the latest `list` / snapshot.
        #[arg(long)]
        index: usize,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ListFormat {
    Markdown,
    Json,
}

pub async fn run(cmd: PendingDecisionsV2Command) -> Result<()> {
    match cmd {
        PendingDecisionsV2Command::Request {
            job_id,
            role,
            agent_id,
            to_agent_id,
            user_content,
            user_content_file,
            list_label,
            llm_content,
            source_event,
        } => {
            let resolved_content = match (user_content, user_content_file) {
                (Some(c), _) => c,
                (None, Some(path)) => {
                    std::fs::read_to_string(&path)
                        .map_err(|e| anyhow::anyhow!("failed to read --user-content-file {path}: {e}"))?
                }
                (None, None) => bail!("either --user-content or --user-content-file is required"),
            };
            handle_request(job_id, role, agent_id, to_agent_id, resolved_content, list_label, llm_content, source_event)
        }
        PendingDecisionsV2Command::RequestPrompt {
            job_id,
            role,
            agent_id,
            to_agent_id,
            user_content,
            user_content_file,
            list_label,
            llm_content,
            source_event,
        } => {
            let resolved_content = match (user_content, user_content_file) {
                (Some(c), _) => c,
                (None, Some(path)) => {
                    std::fs::read_to_string(&path)
                        .map_err(|e| anyhow::anyhow!("failed to read --user-content-file {path}: {e}"))?
                }
                (None, None) => bail!("either --user-content or --user-content-file is required"),
            };
            handle_request_prompt(job_id, role, agent_id, to_agent_id, resolved_content, list_label, llm_content, source_event)
        }
        PendingDecisionsV2Command::Resolve { user_reply } => handle_resolve(user_reply),
        PendingDecisionsV2Command::ResolveWithSessionkey {
            user_reply, job_id, role, agent_id, to_agent_id, source_event,
        } => handle_resolve_with_sessionkey(user_reply, job_id, role, agent_id, to_agent_id, source_event),
        PendingDecisionsV2Command::ResolvePrompt {
            user_reply, job_id, role, agent_id, to_agent_id, source_event,
        } => handle_resolve_prompt(user_reply, job_id, role, agent_id, to_agent_id, source_event),
        PendingDecisionsV2Command::Pick { index } => handle_pick(index),
        PendingDecisionsV2Command::List { format } => handle_list(format),
        PendingDecisionsV2Command::Cancel { index } => handle_cancel(index),
    }
}

// ─── Handlers ──────────────────────────────────────────────────────────

/// Synchronous direct-push variant of `Request`.
///
/// Branches on `OKX_A2A_IS_CLI`:
/// - `OKX_A2A_IS_CLI=1` (CLI driver mode) → no queue write, no playbook
///   emission; immediately invokes `okx-a2a user decision-request` from inside
///   the CLI. On return the card is already in the user session.
/// - Otherwise (queue mode) → falls back to the same queue-write + playbook
///   emission path as `handle_request`. The LLM still executes the printed
///   `okx-a2a user decision-request` bash block, but the queue lifecycle
///   stays consistent with `Request`.
#[allow(clippy::too_many_arguments)]
fn handle_request_prompt(
    job_id: String,
    role: String,
    agent_id: String,
    to_agent_id: Option<String>,
    user_content: String,
    list_label: String,
    llm_content: Option<String>,
    source_event: Option<String>,
) -> Result<()> {
    let cli_mode_env = std::env::var("OKX_A2A_IS_CLI").unwrap_or_default();
    let cli_mode = cli_mode_env == "1";
    trace_log(&format!(
        "handle_request_prompt {} (OKX_A2A_IS_CLI={:?}): job_id={} role={} agent_id={} to_agent_id={:?}",
        if cli_mode { "CLI_MODE" } else { "QUEUE_MODE" },
        cli_mode_env, job_id, role, agent_id, to_agent_id,
    ));

    if cli_mode {
        let now = Utc::now();
        let entry = PendingEntry {
            job_id,
            role,
            agent_id,
            to_agent_id,
            user_content,
            list_label,
            llm_content_override: llm_content,
            source_event,
            status: Status::Active,
            created_at: now,
            updated_at: now,
        };
        let llm_content = resolve_llm_content_cli(&entry);
        use crate::commands::agent_commerce::task::common::okx_a2a;
        okx_a2a::user_decision_request(&entry.user_content, &llm_content)?;
        println!("Decision request submitted. ");
        return Ok(());
    }

    {
        let now = Utc::now();
        let to_ref = to_agent_id.as_deref();
        let new_entry_template = PendingEntry {
            job_id: job_id.clone(),
            role: role.clone(),
            agent_id: agent_id.clone(),
            to_agent_id: to_agent_id.clone(),
            user_content: user_content.clone(),
            list_label: list_label.clone(),
            llm_content_override: llm_content.clone(),
            source_event: source_event.clone(),
            status: Status::Queued,
            created_at: now,
            updated_at: now,
        };

        let _lock = acquire_lock()?;
        let mut q = read_queue()?;
        let original_created_at = q
            .entries
            .iter()
            .find(|e| entry_matches(e, &job_id, &role, &agent_id, to_ref))
            .map(|e| e.created_at)
            .unwrap_or(now);
        q.entries.retain(|e| !entry_matches(e, &job_id, &role, &agent_id, to_ref));
        q.entries.push(PendingEntry {
            created_at: original_created_at,
            ..new_entry_template
        });
        write_queue_atomic(&q)?;
        // Push synchronously — do not emit a playbook for the LLM. Reuse the
        // same llmContent generator as the CLI-mode branch so resolve behavior
        // stays consistent across modes.
        let entry = q.entries.last().unwrap();
        let llm_content = resolve_llm_content_prompt_user(entry);
        use crate::commands::agent_commerce::task::common::okx_a2a;
        okx_a2a::user_decision_request(&entry.user_content, &llm_content)?;
        println!("Decision request submitted (queued for tracking). ");
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_request(
    job_id: String,
    role: String,
    agent_id: String,
    to_agent_id: Option<String>,
    user_content: String,
    list_label: String,
    llm_content: Option<String>,
    source_event: Option<String>,
) -> Result<()> {
    handle_request_prompt(
        job_id,
        role,
        agent_id,
        to_agent_id,
        user_content,
        list_label,
        llm_content,
        source_event,
    )
}

/// CLI-driver bypass: build the full system-shaped relay envelope from the
/// caller-supplied routing fields and dispatch it in-process via
/// `okx_a2a::session_send`. Mirrors the queue-based `handle_resolve`
/// envelope shape exactly (same fields, same `user_decision_<source_event>`
/// event), so the receiving sub routes via the same
/// `next-action --event user_decision_<X>` handler regardless of mode.
fn handle_resolve_with_sessionkey(
    user_reply: String,
    job_id: String,
    role: String,
    agent_id: String,
    to_agent_id: Option<String>,
    source_event: String,
) -> Result<()> {
    use crate::commands::agent_commerce::task::common::okx_a2a;
    trace_log(&format!(
        "handle_resolve_with_sessionkey: job_id={} role={} agent_id={} to_agent_id={:?} source_event={} user_reply={:?}",
        job_id, role, agent_id, to_agent_id, source_event, user_reply,
    ));
    let relay_event = format!("user_decision_{}", source_event);
    let description = format!(
        "User-decision relay envelope (CLI mode). Call `onchainos agent next-action \
         --role {role} --agentId {agent} \
         --message '{{\"event\":\"{evt}\",\"jobId\":\"{jid}\",\"data\":\"<message.data verbatim>\"}}'` \
         to fetch the routing playbook; follow it. \
         ❌ Do NOT call `pending-decisions-v2 resolve` / `pick` / `cancel` — those are \
         user-session-only; the user-session already issued this relay envelope.",
        jid = job_id, evt = relay_event, role = role, agent = agent_id,
    );
    let relay_envelope = serde_json::json!({
        "agentId": agent_id,
        "message": {
            "event": relay_event,
            "data": user_reply,
            "code": 0,
            "description": description,
            "source": "system",
            "jobId": job_id,
            "role": role,
            "timestamp": Utc::now().timestamp(),
        }
    });
    let relay_content = serde_json::to_string(&relay_envelope)
        .unwrap_or_else(|_| format!(
            "{{\"agentId\":\"{}\",\"message\":{{\"event\":\"{}\",\"data\":{:?},\"source\":\"system\",\"jobId\":\"{}\",\"role\":\"{}\"}}}}",
            agent_id, relay_event, user_reply, job_id, role,
        ));
    okx_a2a::session_send(&job_id, to_agent_id.as_deref(), &relay_content)?;
    print!(
        "▶️ **Resume watching** — re-enter the watch loop per `skills/okx-task-watch/SKILL.md` (preserve the session's sticky `--job-id` if it was started post-publish).\n"
    );
    Ok(())
}

/// Queue-backed variant of `handle_resolve_with_sessionkey`. Builds the same
/// system-shaped relay envelope from the caller-supplied routing fields,
/// dispatches it in-process via `okx_a2a::session_send`, and best-effort
/// removes the matching queue entry. Pairs with `playbook_push_prompt_user`
/// so a queue-mode push lands a queue-mode relay.
fn handle_resolve_prompt(
    user_reply: String,
    job_id: String,
    role: String,
    agent_id: String,
    to_agent_id: Option<String>,
    source_event: String,
) -> Result<()> {
    use crate::commands::agent_commerce::task::common::okx_a2a;
    trace_log(&format!(
        "handle_resolve_prompt: job_id={} role={} agent_id={} to_agent_id={:?} source_event={} user_reply={:?}",
        job_id, role, agent_id, to_agent_id, source_event, user_reply,
    ));
    let relay_event = format!("user_decision_{}", source_event);
    let description = format!(
        "User-decision relay envelope (queue-backed prompt mode). Call `onchainos agent next-action \
         --role {role} --agentId {agent} \
         --message '{{\"event\":\"{evt}\",\"jobId\":\"{jid}\",\"data\":\"<message.data verbatim>\"}}'` \
         to fetch the routing playbook; follow it. \
         ❌ Do NOT call `pending-decisions-v2 resolve` / `resolve-with-sessionkey` / `resolve-prompt` / `pick` / `cancel` — those are user-session-only; the user-session already issued this relay envelope.",
        jid = job_id, evt = relay_event, role = role, agent = agent_id,
    );
    let relay_envelope = serde_json::json!({
        "agentId": agent_id,
        "message": {
            "event": relay_event,
            "data": user_reply,
            "code": 0,
            "description": description,
            "source": "system",
            "jobId": job_id,
            "role": role,
            "timestamp": Utc::now().timestamp(),
        }
    });
    let relay_content = serde_json::to_string(&relay_envelope)
        .unwrap_or_else(|_| format!(
            "{{\"agentId\":\"{}\",\"message\":{{\"event\":\"{}\",\"data\":{:?},\"source\":\"system\",\"jobId\":\"{}\",\"role\":\"{}\"}}}}",
            agent_id, relay_event, user_reply, job_id, role,
        ));

    // Best-effort remove the matching entry from the queue (paired with the
    // `handle_request` non-CLI write path). If lock / IO fails, log + continue —
    // the in-process relay below is the critical path and must still happen.
    let to_ref = to_agent_id.as_deref();
    match acquire_lock() {
        Ok(_lock) => match read_queue() {
            Ok(mut q) => {
                let before = q.entries.len();
                q.entries.retain(|e| !entry_matches(e, &job_id, &role, &agent_id, to_ref));
                if q.entries.len() != before {
                    if let Err(e) = write_queue_atomic(&q) {
                        trace_log(&format!("handle_resolve_prompt: write_queue_atomic failed: {e}"));
                    }
                }
            }
            Err(e) => trace_log(&format!("handle_resolve_prompt: read_queue failed: {e}")),
        },
        Err(e) => trace_log(&format!("handle_resolve_prompt: acquire_lock failed: {e}")),
    }

    okx_a2a::session_send(&job_id, to_ref, &relay_content)?;
    print!(
        "🛑 User reply relayed and consumed — do NOT reuse it (no `resolve-prompt` retry, no future-card reference); wait for a fresh user message, then end the turn.\n"
    );
    Ok(())
}

fn handle_resolve(user_reply: String) -> Result<()> {
    use crate::commands::agent_commerce::task::common::okx_a2a;
    let _lock = acquire_lock()?;
    let mut q = read_queue()?;
    ensure_invariant_and_evict(&mut q);

    let active_idx = q.entries.iter().position(|e| e.status == Status::Active);
    let Some(active_idx) = active_idx else {
        // Two sub-cases to distinguish, otherwise we silently swallow user decisions:
        //   a) Truly empty queue → the reply IS just normal chat; end the turn.
        //   b) Selection mode (0 active + N queued, after a prior resolve consumed the
        //      last active and left ≥2 queued): the user's reply belongs to one of the
        //      pending decisions but they haven't picked which yet. Returning
        //      "this is normal chat" here was the bug — it told master to drop the reply,
        //      so the queued subs never got their relay. Instead, refresh the snapshot
        //      and ask the user to pick via stale_relist.
        if q.entries.iter().any(|e| e.status == Status::Queued) {
            let new_snap = build_snapshot(&q);
            write_snapshot_atomic(&new_snap)?;
            print!(
                "{}",
                playbook_stale_relist(
                    &new_snap,
                    "queue is in selection mode — please pick a number first, then re-send your decision"
                )
            );
        } else {
            print!("{}", playbook_error_no_active());
        }
        return Ok(());
    };

    let active = q.entries.remove(active_idx);
    // Relay content is a system-shaped envelope: same JSON skeleton the chain
    // uses for events (`source: "system"`, `event`, `jobId`, ...), so the
    // receiving sub session can dispatch it via its existing `next-action`
    // handler without any string-prefix parsing or keyword-mapping.
    //
    // event = "user_decision_<source_event>" (e.g. "user_decision_job_submitted").
    // If --source-event was not provided at request time, falls back to the
    // generic "user_decision" — sub handles via a default branch.
    let relay_event = match &active.source_event {
        Some(se) => format!("user_decision_{}", se),
        None => "user_decision".to_string(),
    };
    // Description carries explicit routing instructions for the receiving sub agent.
    // Sub LLM tends to read `description` first; making it action-oriented prevents the
    // common mis-routing pattern where the sub pattern-matches "I see user_decision_*"
    // → "this is from resolve flow" → "I should call resolve too" (which is wrong; resolve
    // is user-session-only — user-session ALREADY called it to produce THIS envelope).
    let description = format!(
        "User-decision relay envelope (sub session). Call `onchainos agent next-action \
         --role {role} --agentId {agent} \
         --message '{{\"event\":\"{evt}\",\"jobId\":\"{jid}\",\"data\":\"<message.data verbatim>\"}}'` \
         to fetch the routing playbook; follow it. \
         ❌ Do NOT call `pending-decisions-v2 resolve` / `pick` / `cancel` — those are \
         user-session-only; the user-session already called `resolve` to produce this \
         envelope. The sub session has no queue file; calling resolve here = wasted turn \
         + flow stall.",
        jid = active.job_id,
        evt = relay_event,
        role = active.role,
        agent = active.agent_id,
    );
    let relay_envelope = serde_json::json!({
        "agentId": active.agent_id,
        "message": {
            "event": relay_event,
            "data": user_reply,
            "code": 0,
            "description": description,
            "source": "system",
            "jobId": active.job_id,
            "role": active.role,
            "timestamp": Utc::now().timestamp(),
        }
    });
    let relay_content = serde_json::to_string(&relay_envelope)
        .unwrap_or_else(|_| format!(
            "{{\"agentId\":\"{}\",\"message\":{{\"event\":\"{}\",\"data\":\"{}\",\"source\":\"system\",\"jobId\":\"{}\"}}}}",
            active.agent_id, relay_event, user_reply, active.job_id
        ));

    let queued: Vec<&PendingEntry> = q
        .entries
        .iter()
        .filter(|e| e.status == Status::Queued)
        .collect();

    if queued.is_empty() {
        // Nothing left to advance to — just relay and end the turn.
        okx_a2a::session_send(&active.job_id, active.to_agent_id.as_deref(), &relay_content)?;
        write_queue_atomic(&q)?;
        print!(
            "🛑 User reply relayed and consumed — do NOT reuse it for future cards; wait for a fresh user message, then end the turn.\n"
        );
    } else {
        // Auto-advance: promote the newest queued entry (LIFO — sort already placed it at
        // index 0 since the active was just removed). Render the new active + the remaining
        // list in one go so the user sees the next decision immediately, no extra round-trip
        // through "selection mode".
        //
        // Promote by composite key (not by raw position) to be robust against any reordering.
        let promote = queued[0].clone();
        let promote_to_ref = promote.to_agent_id.as_deref();
        let promote_idx = q
            .entries
            .iter()
            .position(|e| entry_matches(e, &promote.job_id, &promote.role, &promote.agent_id, promote_to_ref))
            .unwrap();
        q.entries[promote_idx].status = Status::Active;
        // Re-sort so the newly-promoted active sits at index 0 (the sort honors the
        // "active first, then LIFO" invariant).
        ensure_invariant_and_evict(&mut q);

        let snap = build_snapshot(&q);
        write_snapshot_atomic(&snap)?;
        write_queue_atomic(&q)?;

        okx_a2a::session_send(&active.job_id, active.to_agent_id.as_deref(), &relay_content)?;
        print!("{}", playbook_advance_only(&q));
    }
    Ok(())
}

fn handle_pick(index: usize) -> Result<()> {
    let _lock = acquire_lock()?;
    let mut q = read_queue()?;
    ensure_invariant_and_evict(&mut q);

    let snapshot = read_snapshot();
    if index == 0 || index > snapshot.items.len() {
        let new_snap = build_snapshot(&q);
        write_snapshot_atomic(&new_snap)?;
        print!("{}", playbook_stale_relist(&new_snap, "selection index out of range"));
        return Ok(());
    }

    let target = snapshot.items[index - 1].clone();
    let target_to = target.to_agent_id.as_deref();
    let snap_displayed_at = snapshot.displayed_at;

    let entry_idx = q.entries.iter().position(|e| entry_matches(e, &target.job_id, &target.role, &target.agent_id, target_to));
    let Some(entry_idx) = entry_idx else {
        let new_snap = build_snapshot(&q);
        write_snapshot_atomic(&new_snap)?;
        print!(
            "{}",
            playbook_stale_relist(&new_snap, "selected entry no longer exists (auto-cleaned or resolved)")
        );
        return Ok(());
    };

    // Stale-selection check: entry was overwritten after snapshot was taken
    if let Some(displayed_at) = snap_displayed_at {
        if q.entries[entry_idx].updated_at > displayed_at {
            let new_snap = build_snapshot(&q);
            write_snapshot_atomic(&new_snap)?;
            print!(
                "{}",
                playbook_stale_relist(&new_snap, "selected entry's content was updated since display")
            );
            return Ok(());
        }
    }

    // New behaviour (all-Queued model): pick is render-only — no status mutation, no swap,
    // no auto-promote. We just render the selected card so the user can see its full
    // content. The previous Active/Queued promotion logic was removed because nothing
    // downstream (handle_resolve_prompt) reads Status::Active anymore.
    print!("{}", playbook_render(&q.entries[entry_idx]));
    Ok(())
}

fn handle_cancel(index: usize) -> Result<()> {
    let _lock = acquire_lock()?;
    let mut q = read_queue()?;
    ensure_invariant_and_evict(&mut q);

    // Resolve target via the snapshot's (jobId, role, agentId, toAgentId?) tuple.
    let snapshot = read_snapshot();
    if index == 0 || index > snapshot.items.len() {
        let new_snap = build_snapshot(&q);
        write_snapshot_atomic(&new_snap)?;
        print!(
            "{}",
            playbook_stale_relist(&new_snap, "cancel index out of range")
        );
        return Ok(());
    }
    let target = snapshot.items[index - 1].clone();
    let target_to = target.to_agent_id.as_deref();

    // Locate + remove
    let Some(entry_idx) = q.entries.iter().position(|e| entry_matches(e, &target.job_id, &target.role, &target.agent_id, target_to)) else {
        print!(
            "{}",
            playbook_error(&format!(
                "no pending decision found for index {} (jobId={} role={} agentId={} toAgentId={:?})",
                index, target.job_id, target.role, target.agent_id, target.to_agent_id,
            ))
        );
        return Ok(());
    };
    let removed = q.entries.remove(entry_idx);
    let was_active = removed.status == Status::Active;

    // If we just cancelled the active and there's at least one queued left, auto-promote
    // the newest queued (LIFO) so the user keeps a clean "current focus" without round-tripping
    // through selection mode.
    if was_active && !q.entries.is_empty() {
        let newest_queued_key = q
            .entries
            .iter()
            .filter(|e| e.status == Status::Queued)
            .max_by_key(|e| e.created_at)
            .map(|e| (e.job_id.clone(), e.role.clone(), e.agent_id.clone(), e.to_agent_id.clone()));
        if let Some((j, r, a, t)) = newest_queued_key {
            if let Some(promote_idx) = q.entries.iter().position(|e| entry_matches(e, &j, &r, &a, t.as_deref())) {
                q.entries[promote_idx].status = Status::Active;
                ensure_invariant_and_evict(&mut q);
            }
        }
    }

    // Refresh snapshot so a subsequent `pick --index N` resolves correctly
    // when the user chooses the next decision from the remaining list.
    let snap = build_snapshot(&q);
    write_snapshot_atomic(&snap)?;
    write_queue_atomic(&q)?;

    print!("{}", playbook_cancel(&removed, was_active, &q, &snap));
    Ok(())
}

fn handle_list(format: ListFormat) -> Result<()> {
    let _lock = acquire_lock()?;
    let mut q = read_queue()?;
    let evicted = ensure_invariant_and_evict(&mut q);

    // Refresh snapshot so subsequent `pick --index N` can resolve correctly
    let snap = build_snapshot(&q);
    write_snapshot_atomic(&snap)?;
    write_queue_atomic(&q)?;

    match format {
        ListFormat::Json => {
            let payload = serde_json::json!({
                "evicted_since_last_call": evicted,
                "entries": q.entries.iter().enumerate().map(|(i, e)| serde_json::json!({
                    "index": i + 1,
                    "job_id": e.job_id,
                    "role": e.role,
                    "agent_id": e.agent_id,
                    "to_agent_id": e.to_agent_id,
                    "list_label": e.list_label,
                    "status": match e.status { Status::Active => "active", Status::Queued => "queued" },
                    "created_at": e.created_at.to_rfc3339(),
                    "updated_at": e.updated_at.to_rfc3339(),
                })).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        ListFormat::Markdown => {
            if evicted > 0 {
                let ttl_days = load_global_ttl().as_secs() / (24 * 60 * 60);
                println!(
                    "ℹ️ Since last check, {} decision(s) older than {} days were auto-cleaned.\n",
                    evicted, ttl_days,
                );
            }
            let n = q.entries.len();
            if n == 0 {
                println!("(no pending decisions)\n");
                println!("Render the line above to the user as your assistant response.");
            } else {
                let view = render_list_markdown(&q);
                print!(
                    "3 steps (Steps 1-2 in this turn, Step 3 in the future turn):\n\n\
                     **Step 1** — Translate the [Source content] below to the user's language per [Translation rules].\n\n\
                     **Step 2** — Render Step 1's output to the user as your assistant response.\n\n\
                     **Step 3** — (Future turn) Apply [Future-turn user-reply routing] below when the user replies.\n\n\
                     {view}"
                );
            }
        }
    }
    Ok(())
}

/// Render the `list --format markdown` output: focused-card-with-remaining-list view.
///
/// Two shapes:
///   * **Has active**: render the active card (verbatim user_content) at the top with a 🟢
///     prefix, then a separator + remaining-list (renumbered 1..M starting from the first
///     non-active entry), then the footer ("Reply A/B" / "switch N" / "later").
///   * **Selection mode** (0 active + N queued, post-resolve/post-cancel): render only the
///     numbered list; no active card to highlight. Footer asks user to pick a number.
///
/// Assumes the queue has already been sorted by `ensure_invariant_and_evict` so that — if
/// any active exists — it sits at index 0, and remaining entries follow in LIFO (newest
/// queued first).
/// Renders the components used by every list-view playbook.
///
/// Output layout (no Step labels — caller adds them):
///   [Source to render to user]:
///   <body>
///
///   [Translation rules]:
///   - …
///
///   [Future-turn user-reply routing]:
///   - …
///
/// Callers (`handle_list`, `playbook_advance_only`, `playbook_cancel`) wrap
/// this with their own Step numbering (e.g. "Step 1 — Translate", "Step 2 —
/// Render", "Step N — (Future turn) routing"). The labeled sections act as
/// natural boundaries — no ═══ zone markers needed.
fn render_list_markdown(q: &Queue) -> String {
    let n = q.entries.len();
    let active_idx = q.entries.iter().position(|e| e.status == Status::Active);

    // ── User-visible body ───────────────────────────────────────────────────────
    let mut user_body = String::new();
    if let Some(ai) = active_idx {
        let active = &q.entries[ai];
        user_body.push_str(&format!(
            "🟢 Decision 1 — {label} (Job {job})\n\n{body}\n\n",
            label = strip_label_prefix(&active.list_label),
            job = short_job_id(&active.job_id),
            body = active.user_content,
        ));

        let remaining: Vec<&PendingEntry> = q
            .entries
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != ai)
            .map(|(_, e)| e)
            .collect();
        if !remaining.is_empty() {
            user_body.push_str("─────────────────\n");
            user_body.push_str(&format!("Remaining ({}):\n", remaining.len()));
            for (j, e) in remaining.iter().enumerate() {
                user_body.push_str(&format!(
                    "{n}. {label} (Job {job})\n",
                    n = j + 1,
                    label = strip_label_prefix(&e.list_label),
                    job = short_job_id(&e.job_id),
                ));
            }
            user_body.push('\n');
            user_body.push_str(
                "Reply per the options shown in the active card to handle this decision; reply \"switch N\" to jump to remaining item N; reply \"later\" to defer.\n",
            );
        } else {
            user_body.push_str(
                "Reply per the options shown in the active card to handle this decision; reply \"later\" to defer.\n",
            );
        }
    } else {
        user_body.push_str("Please pick one to activate:\n\n");
        for (i, e) in q.entries.iter().enumerate() {
            user_body.push_str(&format!(
                "{n}. {label} (Job {job})\n",
                n = i + 1,
                label = strip_label_prefix(&e.list_label),
                job = short_job_id(&e.job_id),
            ));
        }
        user_body.push('\n');
        user_body.push_str(&format!(
            "Reply with a number 1-{n} to activate that decision, or \"later\" to defer.\n",
            n = n,
        ));
    }

    // ── Final composition: source body + translation rules + future routing ──
    let mut out = String::new();
    out.push_str("[Source content to render to user]:\n\n");
    out.push_str(&user_body);
    out.push('\n');

    out.push_str(
        "[Translation rules] — **translate every English word to the user's language**, including quoted user-facing keywords. Only these are kept verbatim:\n\
         \x20\x20- Hex jobIds (`0x...`).\n\
         \x20\x20- Sub-provided `<title>` fields (already in user's language).\n\
         \x20\x20- Structural delimiters (`🟢`, `─────────────────`, numbered list markers).\n\
         Everything else — `Decision`, the `<type>` token (`acceptance` / `dispute` / `submit` / `ASP-pick` / `ASP-contact` / `next-step` / `price` / `budget` / `error`), `decision`, all surrounding prose, AND quoted user-facing keywords like `\"switch N\"` / `\"later\"` — gets translated. Footer: preserve every `;`-separated clause (do NOT drop or merge). No mixed-language content.\n\n",
    );

    out.push_str("[Future-turn user-reply routing] (when the user replies, match semantics — localized equivalents count):\n");
    if active_idx.is_some() {
        let remaining_count = q.entries.len() - 1;
        out.push_str(
            "\x20\x20- Reply matches the active card's option set (`A` / `B` / `A`/`B`/`C` / numeric `1`/`2`/`3` / free-form like `retry` / `dismiss` / `重试` / `同意` / `拒绝` / `通过` / `第一个` / etc.) → `onchainos agent pending-decisions-v2 resolve --user-reply \"<user's verbatim wording>\"`\n\
             \x20\x20\x20\x20⚠️ Disambiguation: if the active card uses numeric options (e.g. \"1. Alpha / 2. Beta\"), a bare `1` / `2` is the active answer → use `resolve`, NOT `pick`. `pick` requires explicit `switch` / `切换` / `跳到` keyword.\n",
        );
        if remaining_count > 0 {
            out.push_str(&format!(
                "\x20\x20- `switch N` / `切换 N` / `跳到 N` / `go to N` / `change to N` (1 ≤ N ≤ {m}) → `onchainos agent pending-decisions-v2 pick --index (N+1)` (e.g. `switch 2` → `--index 3`).\n",
                m = remaining_count,
            ));
        }
        out.push_str(
            "\x20\x20- `later` / `稍后` / `defer` → end the turn.\n\
             \x20\x20- User asks to see the list again → `onchainos agent pending-decisions-v2 list --format markdown`.\n\
             \x20\x20- Else → ordinary chat; do NOT call `pick` / `resolve` / `cancel`.\n",
        );
    } else {
        out.push_str(&format!(
            "\x20\x20- A number K (1 ≤ K ≤ {n}) / `第 K 个` / `选 K` / `the Kth` → `onchainos agent pending-decisions-v2 pick --index K`.\n\
             \x20\x20- `later` / `稍后` / `defer` → end the turn.\n\
             \x20\x20- User asks to see the list again → `onchainos agent pending-decisions-v2 list --format markdown`.\n\
             \x20\x20- Else → ordinary chat. No active entry to resolve.\n",
            n = n,
        ));
    }

    out
}

fn build_snapshot(q: &Queue) -> DisplaySnapshot {
    DisplaySnapshot {
        displayed_at: Some(Utc::now()),
        items: q
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| DisplayItem {
                index: i + 1,
                job_id: e.job_id.clone(),
                role: e.role.clone(),
                agent_id: e.agent_id.clone(),
                to_agent_id: e.to_agent_id.clone(),
                list_label: e.list_label.clone(),
            })
            .collect(),
    }
}

fn short_job_id(job_id: &str) -> String {
    if job_id.len() <= 12 {
        job_id.to_string()
    } else {
        format!("{}...{}", &job_id[..6], &job_id[job_id.len() - 4..])
    }
}

/// Strip the leading bracketed prefix (e.g. `[Decision 0x3938…815d]`) from a
/// list_label so the natural-language description stands alone in the
/// reprompt notification. Returns the input unchanged if it doesn't start
/// with a `[…]` block.
fn strip_label_prefix(label: &str) -> &str {
    if label.starts_with('[') {
        if let Some(end) = label.find(']') {
            return label[end + 1..].trim_start();
        }
    }
    label
}

/// Canonical sub-side bash + immediate hints for `pending-decisions-v2 request` —
/// shared by every scene that pushes a decision to the user-session queue.
///
/// Encapsulates ONLY the 5-substep procedure (sessionKey → translate → run request →
/// follow stdout playbook → end turn). Callers retain their own preamble (state /
/// role / absolute-requirement) and any post-procedure warnings.
///
/// The "follow the playbook the CLI returns" line is hardened here vs. the previous
/// hand-written copies: it spells out the three possible stdout shapes
/// (`okx-a2a user decision-request` / `okx-a2a user notify` / end-turn) and explicitly
/// warns that stdout IS the next-action playbook (not log output). Without this, smaller models
/// tend to stop after the bash call — the user-facing tool invocation never happens,
/// the card never surfaces, the flow stalls (24h auto-refund / mistaken auto-decline).
///
/// Arguments:
/// - `job_id`: full hex jobId
/// - `role`: `buyer` | `provider` | `evaluator`
/// - `agent_id`: numeric agentId (string form)
/// - `user_content`: the user-facing prompt body (canonical English; LLM localizes
///   before pasting). Double-quote (`"`) and backslash safety is handled internally.
/// - `list_label_full`: full label INCLUDING bracket prefix
///   (e.g. `[Decision <short_id>] <title> dispute decision`).
/// - `source_event`: bare event name (e.g. `job_rejected`); becomes the
///   `<source_event>` token in the relay envelope's
///   `event = user_decision_<source_event>` after the user replies.
pub fn request_command_block(
    job_id: &str,
    role: &str,
    agent_id: &str,
    to_agent_id: Option<&str>,
    user_content: &str,
    list_label_full: &str,
    source_event: &str,
) -> String {
    // Bash `--user-content "..."` uses double quotes; escape `\` and `"` inside.
    let user_content_escaped = user_content
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let to_flag = match to_agent_id {
        Some(t) => format!(" --to-agent-id \"{t}\""),
        None => String::new(),
    };
    format!(
        "Execute these 3 sub-steps strictly in order. ALL THREE are mandatory; skipping any breaks the flow.\n\n\
         **(1) Translate `--user-content` AND `--list-label` to the user's language BEFORE step (2)**. The bash in (2) shows English placeholders — the actual strings you pass MUST be localized per the rules below.\n\
         \x20\x20• **Language signal** = user's OWN typed messages in THIS session ONLY. Task title / description / peer's message / playbook examples are NOT signals (even if they contain non-English text). Unsure → default English.\n\
         \x20\x20• **Translate EVERY user-visible word** — outer prose, text inside single-quotes, placeholder words inside `<...>`, AND task title. The only thing kept verbatim is the shortJobId hex (it's an identifier, not language).\n\
         \x20\x20• **No mixed-language in any single string**.\n\n\
         **(2) Run `pending-decisions-v2 request`** with translated args from (1):\n\
         ```bash\n\
         onchainos agent pending-decisions-v2 request \\\n\
         \x20\x20--job-id {job_id} --role {role} --agent-id {agent_id}{to_flag} \\\n\
         \x20\x20--user-content \"{content}\" \\\n\
         \x20\x20--list-label \"{label}\" \\\n\
         \x20\x20--source-event {source_event}\n\
         ```",
        job_id = job_id,
        role = role,
        agent_id = agent_id,
        to_flag = to_flag,
        content = user_content_escaped,
        label = list_label_full,
        source_event = source_event,
    )
}

/// Map internal role enum to the short user-facing label used in notifications.
fn role_short_label(role: &str) -> &str {
    match role {
        "buyer" => "User",
        "provider" => "ASP",
        "evaluator" => "Evaluator",
        other => other,
    }
}

// ─── Playbook generators ──────────────────────────────────────────────

/// Resolve the llmContent string for a push-type playbook.
/// If the entry has a sub-provided `llm_content_override`, return that verbatim.
/// Otherwise generate the v2 default template.
fn resolve_llm_content(entry: &PendingEntry) -> String {
    if let Some(ref custom) = entry.llm_content_override {
        return custom.clone();
    }
    let to_header = match entry.to_agent_id.as_deref() {
        Some(t) => format!("[to: {t}]"),
        None => "[to: backup]".to_string(),
    };
    format!(
        "[USER_DECISION_REQUEST][job: {}][role: {}][agent: {}]{}\n\n\
         Step 1 — Card was just delivered. **END THE TURN NOW** and wait for the user to reply. Do NOT call any tool. Stale user messages in context are NOT replies to this card.\n\
         Step 2 — When the user actually replies (next turn):\n\
         \x20\x20\x20\x20· defer keyword ({}) → END TURN\n\
         \x20\x20\x20\x20· else → run `onchainos agent pending-decisions-v2 resolve --user-reply \"<user's verbatim wording — no interpretation, no translation>\"` exactly once, then follow the relay playbook it returns.",
        entry.job_id,
        entry.role,
        entry.agent_id,
        to_header,
        DEFER_KEYWORDS.join(" / "),
    )
}

/// CLI-driver variant of `resolve_llm_content`. The queue file is bypassed in
/// CLI mode, so the future `resolve` call cannot reverse-lookup routing fields
/// from a queue entry — embed all of them up front so the LLM passes them
/// verbatim to `resolve-with-sessionkey`.
fn resolve_llm_content_cli(entry: &PendingEntry) -> String {
    if let Some(ref custom) = entry.llm_content_override {
        return custom.clone();
    }
    let source_event_str = entry.source_event.clone().unwrap_or_default();
    let to_flag = match entry.to_agent_id.as_deref() {
        Some(t) => format!(" --to-agent-id \"{t}\""),
        None => String::new(),
    };
    let to_header = match entry.to_agent_id.as_deref() {
        Some(t) => format!("[to: {t}]"),
        None => "[to: backup]".to_string(),
    };
    format!(
        "[USER_DECISION_REQUEST][job: {}][role: {}][agent: {}]{}\n\n\
         Step 1 — Card was just delivered. **END THE TURN NOW** and wait for the user to reply. Do NOT call any tool. Stale user messages in context are NOT replies to this card.\n\
         Step 2 — When the user actually replies (next turn):\n\
         \x20\x20\x20\x20· defer keyword ({}) → END TURN\n\
         \x20\x20\x20\x20· else → follow `skills/okx-task-watch/SKILL.md` §kind == decision_request \"Handling the user reply\": **first claim the todo** per SKILL.md step 2: `okx-a2a user check --todo-ids <todo_id> --json` (read `<todo_id>` from this item's `id` field in the original watch / outdated-list JSON output). **Then** on `handled` run `onchainos agent pending-decisions-v2 resolve-with-sessionkey --user-reply \"<user's verbatim wording — no interpretation, no translation>\" --job-id \"{}\" --role \"{}\" --agent-id \"{}\"{} --source-event \"{}\"` exactly once, then follow the relay playbook it returns. Skipping the `check` leaves a ghost todo in the outstanding-decisions queue.",
        entry.job_id,
        entry.role,
        entry.agent_id,
        to_header,
        DEFER_KEYWORDS.join(" / "),
        entry.job_id,
        entry.role,
        entry.agent_id,
        to_flag,
        source_event_str,
    )
}

/// Variant of `resolve_llm_content_cli` for the `playbook_push_prompt_user`
/// (non-OKX_A2A_IS_CLI) path. Adds a multi-decision disambiguation branch in
/// Step 2 so that when multiple [USER_DECISION_REQUEST] blocks coexist in the
/// LLM's context, the LLM first asks the user which jobId they're answering
/// rather than guessing.
fn resolve_llm_content_prompt_user(entry: &PendingEntry) -> String {
    if let Some(ref custom) = entry.llm_content_override {
        return custom.clone();
    }
    let source_event_str = entry.source_event.clone().unwrap_or_default();
    let to_flag = match entry.to_agent_id.as_deref() {
        Some(t) => format!(" --to-agent-id \"{t}\""),
        None => String::new(),
    };
    let to_header = match entry.to_agent_id.as_deref() {
        Some(t) => format!("[to: {t}]"),
        None => "[to: backup]".to_string(),
    };
    format!(
        "[USER_DECISION_REQUEST]\n\
         [job: {job}][role: {role}][agent: {agent}]{to_header}\n\
         (Anything above this marker is stale — NOT a reply to this card.)\n\n\
         Step 1 — Card just delivered.\n\n\
         Step 2 — Scan your current context for OTHER [USER_DECISION_REQUEST] blocks. \
         If you find any, render the warning below to the user as your assistant response (in user's language), e.g.:\n\
         \x20\x20`⚠️ You have multiple decisions pending — please prefix your reply with the jobId short hash, e.g. \\`0x7091: approve\\`, so it routes correctly.`\n\
         If no other blocks → skip this step.\n\n\
         Step 3 — **END THE TURN NOW**, wait for user reply.\n\n\
         🛑 **The block below runs ONLY in a future turn**, AFTER the user has actually replied. Do NOT run anything in the current turn.\n\
         On the user's next reply, re-scan your context for [USER_DECISION_REQUEST] blocks (the count may have changed since Step 2), then walk this decision tree:\n\
         \x20\x20· defer keyword ({defer}) → END TURN, do NOT run anything.\n\
         \x20\x20· Reply starts with `0x...:` prefix → strip the prefix + colon, use the prefix to match each block's `[job: 0x...]` header, locate THAT block, then run THAT block's command template with `--user-reply` set to the stripped wording (without the prefix).\n\
         \x20\x20· No prefix + only THIS block in context (single) → run THIS block's command template with the full reply.\n\
         \x20\x20· 🔁 No prefix + **multiple** [USER_DECISION_REQUEST] blocks in context → user forgot to add the jobId prefix. Ask them which jobId they're answering (number the candidates `1. Job 0x...`, `2. Job 0x...`, one per line — short_jobId only), **END THE TURN**, wait for the pick (hex prefix `0x7091` or list number `1`); locate THAT block via `[job: 0x...]` header (or list order), then run THAT block's command template. Never guess, never collapse.\n\n\
         **Command template** (pre-filled for THIS block; only run AFTER the user has replied):\n\
         \x20\x20`onchainos agent pending-decisions-v2 resolve-prompt --user-reply \"<user wording, without any jobId prefix>\" --job-id \"{job}\" --role \"{role}\" --agent-id \"{agent}\"{to_flag} --source-event \"{src}\"`\n\n\
         After running, follow the relay playbook the command returns.",
        job = entry.job_id,
        role = entry.role,
        agent = entry.agent_id,
        to_header = to_header,
        to_flag = to_flag,
        src = source_event_str,
        defer = DEFER_KEYWORDS.join(" / "),
    )
}

fn playbook_wait(position: usize) -> String {
    format!(
        "The user is currently answering a prior decision. Your decision is queued (position {}).\n\n\
         Do NOT call any `okx-a2a` user / session command. End the turn now. The CLI will auto-render your prompt when it becomes active.\n",
        position
    )
}

/// Queued + re-prompt: a genuinely new sub_key landed in the queue. Re-surface
/// **the ACTIVE decision's full content** to the user (it may have scrolled off
/// under intermediate chat), with a short "another decision queued" notice on
/// top. The user-visible decision in this prompt is the ACTIVE one — answering
/// it routes to the active sub via `resolve` (single-active invariant). The new
/// queued entry is only mentioned by its label as a heads-up; its full content
/// will auto-display later when the active resolves.
///
/// Why this design: the user complained that an earlier variant which showed
/// the NEW (queued) decision's full content + told the user "answer the active
/// first" was confusing — the user reads the visible decision and replies to
/// it, but resolve routes to a DIFFERENT (active) decision the user can't see,
/// so the dispatched sessionKey looks "wrong" from the user's perspective.
/// Showing the active's content keeps the visible-decision and routed-decision
/// aligned.
fn playbook_wait_with_reprompt(
    active: &PendingEntry,
    new_entry: &PendingEntry,
    queued_position: usize,
) -> String {
    let total_pending = queued_position + 1;
    // Canonical English notification. The user-session LLM translates the entire
    // body to match the user's language before `okx-a2a user notify`. We do NOT
    // embed the active card content here — the user is already partway through
    // answering it; re-surfacing the full card would be noisy. The user can
    // ask for the decision list to switch focus.
    let _ = active; // active is no longer rendered inline; kept in signature for callers + future use
    let label_clean = strip_label_prefix(&new_entry.list_label);
    let short_job = short_job_id(&new_entry.job_id);
    let role_label = role_short_label(&new_entry.role);
    let dispatch_content = format!(
        "🆕 {label} (Job {job}, {role}: Agent #{agent}) — added to the pending-decisions queue.\n\
         You currently have {n} pending decisions; reply \"decision list\" to view and pick which to handle.",
        label = label_clean,
        job = short_job,
        role = role_label,
        agent = new_entry.agent_id,
        n = total_pending,
    );
    format!(
        "Your decision is queued (position {pos}). 2 steps:\n\n\
         **Step 1 — Localize this content to the user's language** (rules below):\n\
         {body}\n\n\
         [Localization rules] — **translate every English word to the user's language**, including quoted user-facing keywords. Only these are kept verbatim:\n\
         \x20\x20• Hex jobIds (`0x...`) and numeric agent IDs (the digits after `Agent #`).\n\
         \x20\x20• The sub-provided `<title>` field (may already be in user's language).\n\
         Everything else — `Decision`, the `<type>` token (`acceptance` / `dispute` / `submit` / `ASP-pick` / `ASP-contact` / `next-step` / `price` / `budget` / `error`), the role token (`User` / `ASP` / `Evaluator`), surrounding prose, AND quoted user-facing keywords like `\"decision list\"` — gets translated to a natural localized form (skill routing accepts both English and translated keywords). No mixed-language content.\n\n\
         **Step 2 — Run `okx-a2a user notify` with the localized content from Step 1**:\n\
         ```bash\n\
         okx-a2a user notify --content '<the localized Step 1 output>'\n\
         ```\n\n\
         End the turn after the command returns. Do NOT call any other tool first or after.\n",
        pos = queued_position,
        body = dispatch_content,
    )
}

/// Resolve auto-advance playbook: render the next decision (auto-promoted newest queued)
/// using the unified list view.
///
/// Used whenever ≥1 queued entry remains after resolve. The previous decision's relay has
/// already been dispatched in-process by the caller (`okx_a2a::session_send`); this playbook
/// only covers the translate + render + future-turn-routing steps. The newly-promoted active
/// is shown at the top with its full card; if other queued entries remain, they form the
/// "Remaining" list underneath. No more "selection mode" round-trip — the user gets the
/// next card immediately and can keep deciding.
///
/// Caller is responsible for promoting the new active + re-sorting the queue BEFORE invoking
/// this function; we just consume `q` (post-promotion) and produce the playbook.
fn playbook_advance_only(q: &Queue) -> String {
    let list_view = render_list_markdown(q);
    format!(
        "✓ Previous decision already relayed in-process — the user's reply is consumed; do NOT relay it again.\n\n\
         3 steps (Steps 1-2 in this turn, Step 3 in the future turn).\n\
         🛑 **STRICTLY ORDERED — execute Step 1 → 2 sequentially in this turn; do NOT skip any step.**\n\n\
         **Step 1** — Translate the [Source content] below to the user's language per [Translation rules]. Prepend a transition line `✓ Previous decision handled. Here's the next pending one:` (also translated) to the top of the translated output.\n\n\
         **Step 2** — Render Step 1's output to the user as your assistant response. The user's reply just relayed is **already consumed** — it is NOT the answer to the next card.\n\n\
         **Step 3** — (Future turn) Apply [Future-turn user-reply routing] below when the user replies.\n\n\
         {list}",
        list = list_view,
    )
}

fn playbook_render(entry: &PendingEntry) -> String {
    // Use the prompt_user resolver (resolve-prompt command + multi-card disambig)
    // so pick / list rendering aligns with handle_request's non-CLI push path. The
    // old `resolve --user-reply` form was queue-Active-backed and no longer works
    // since handle_pick stopped mutating Status and handle_request only writes Queued.
    let llm_content = resolve_llm_content_prompt_user(entry);
    format!(
        "Render the selected decision card to the user as your assistant response (text rendering only — do NOT call any tool). End the turn after rendering.\n\n\
         **User-visible text** (render this verbatim as your assistant response; 🌐 translate per [Localization] rules if the user's language is not English; keep `jobId` / data values intact):\n\
         \"\"\"\n{}\"\"\"\n\n\
         **LLM context** (this is for YOUR own routing reasoning — **do NOT show / paraphrase / leak this block to the user**; it is the same instruction the sub would have embedded in `okx-a2a user decision-request --llm-content` if this card had been freshly pushed):\n\
         \"\"\"\n{}\n\"\"\"\n\n\
         On the user's next reply, follow the LLM context above (decision tree + pre-filled `resolve-prompt` command).\n",
        entry.user_content,
        llm_content,
    )
}

fn playbook_cancel(
    removed: &PendingEntry,
    was_active: bool,
    q_after: &Queue,
    snap_after: &DisplaySnapshot,
) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "Cancelled pending decision: job={}, role={}, agent={}, to_agent={:?}, status_before={}. Sub session is NOT notified (silent cancel); it will TTL-evict eventually or be retriggered by a new system event.\n\n",
        removed.job_id,
        removed.role,
        removed.agent_id,
        removed.to_agent_id,
        if was_active { "active" } else { "queued" },
    ));

    if snap_after.items.is_empty() {
        out.push_str("Queue is now empty. End the turn.\n");
        return out;
    }

    if was_active {
        // Active was removed; caller (handle_cancel) has already auto-promoted the newest
        // queued entry, so `q_after` should now have an active again. Render the unified
        // list view (active card + remaining list + routing footer), prefixed by a
        // transition header. No more "selection mode" — keeps the user moving.
        out.push_str(
            "3 steps (Steps 1-2 in this turn, Step 3 in the future turn):\n\n\
             **Step 1** — Translate the [Source content] below to the user's language per [Translation rules]. Prepend a transition line `✓ Previous decision cancelled. Here's the next pending one:` (also translated) to the top of the translated output.\n\n\
             **Step 2** — Render Step 1's output to the user as your assistant response.\n\n\
             **Step 3** — (Future turn) Apply [Future-turn user-reply routing] below when the user replies.\n\n",
        );
        out.push_str(&render_list_markdown(q_after));
    } else {
        out.push_str("Active entry was NOT affected (the cancelled entry was queued, not active). End the turn.\n");
    }

    out
}

fn playbook_error_no_active() -> String {
    // Reached only when the queue is truly empty (0 active + 0 queued).
    // Selection-mode (0 active + N>0 queued) is handled separately in handle_resolve
    // and returns a stale_relist playbook instead.
    "The pending-decisions queue is empty — there is no decision to resolve. \
     The user's reply is just a normal chat message; handle it as such.\n\
     Do NOT call any `okx-a2a` user / session command. End the turn now.\n"
        .to_string()
}

fn playbook_error(msg: &str) -> String {
    format!(
        "Cannot proceed: {}\nDo NOT call any `okx-a2a` user / session command. End the turn.\n",
        msg
    )
}

fn playbook_stale_relist(snap: &DisplaySnapshot, reason: &str) -> String {
    let mut list = String::new();
    if snap.items.is_empty() {
        list.push_str("Queue is empty, no selection needed.\n");
    } else {
        list.push_str(&format!(
            "Your previous selection is stale ({}). Current list:\n\n",
            reason
        ));
        for it in &snap.items {
            list.push_str(&format!("{}. {}\n", it.index, it.list_label));
        }
        list.push_str(&format!("\nReply with a number 1-{} to re-select.\n", snap.items.len()));
    }
    format!(
        "The previous selection is stale. In your assistant response, render the following list VERBATIM:\n\n\
         \"\"\"\n{}\"\"\"\n\n\
         After rendering, end the turn. Do NOT call any tool.\n",
        list
    )
}

fn indent(s: &str, prefix: &str) -> String {
    s.lines()
        .map(|l| format!("{}{}", prefix, l))
        .collect::<Vec<_>>()
        .join("\n")
}