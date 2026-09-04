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

pub use crate::commands::agent_commerce::task::common::config::is_cli_mode;
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

/// Defer vocabulary embedded in the generated user-session instructions.
/// The CLI does not parse these values itself. A defer reply keeps the decision
/// unclaimed; watch continuation depends only on the card's active-watch origin.
pub const DEFER_KEYWORDS: &[&str] = &[
    // Chinese
    "等会儿",
    "等等",
    "等一下",
    "稍后",
    "晚点",
    "先放着",
    "先不管",
    "回头再看",
    // English
    "skip",
    "later",
    "wait",
    "hold on",
    "not now",
    "defer",
];

/// Post-relay instruction shared by both direct CLI and queue-backed resolvers.
/// Whether a decision resumes watch is a property of how the card was surfaced,
/// never of reply text such as A/B/C, an amount, a cap, or a defer keyword.
fn decision_relay_post_action() -> &'static str {
    "Decision relayed. If this card was surfaced by a currently active `okx-a2a user watch`, immediately re-enter that exact originating watch command per `skills/okx-ai/references/watch-core.md` (preserve global vs sticky `--job-id`). If it was opened independently through a decision list / outdated-list, do not start watch; end the turn normally. Never infer watch origin from the user's reply text.\n"
}

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
            let home = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("unable to determine HOME directory"))?;
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
    let mut queue = serde_json::from_str::<Queue>(&raw).unwrap_or_default();
    // Delivery-time execution-mode/configuration prompts were removed in
    // 4.8.x. Absorb locally persisted cards from older binaries so list, pick,
    // cancel, and auto-promotion cannot surface or extend them after an upgrade.
    queue.entries.retain(|entry| {
        !crate::commands::agent_commerce::task::common::autotrade::is_retired_mode_configuration_decision(
            entry.source_event.as_deref(),
        )
    });
    Ok(queue)
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
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent dir"))?;
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
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent dir"))?;
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
        /// Base64(JSON object) of whitelisted template variables (e.g. the untrusted task
        /// title under key `__OKX_TASK_TITLE__`). Optional; when present, {{KEY}} placeholders
        /// in --user-content / --list-label are decoded, validated and literally replaced
        /// in-process after parsing. Never contains executable shell; logged redacted.
        #[arg(long = "template-vars-b64")]
        template_vars_b64: Option<String>,
    },

    /// (user-session) Resolve the current active decision with user's reply.
    Resolve {
        #[arg(long = "user-reply")]
        user_reply: String,
    },

    /// (user-session, CLI-driver bypass) Resolve a decision without consulting
    /// the queue file — caller passes every routing field explicitly so the
    /// envelope can be built and dispatched. Pairs with `request`'s
    /// `is_cli_mode()` bypass; used when a CLI driver loop (Claude Code / Codex)
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
        /// Strict candidate JSON extracted by the foreground model for the
        /// auto-trade consent/config decision. The CLI validates this object;
        /// it never parses the user's natural-language reply.
        #[arg(long = "autotrade-candidate-json")]
        autotrade_candidate_json: Option<String>,
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
        /// Queue-mode equivalent of `--autotrade-candidate-json` on
        /// `resolve-with-sessionkey`.
        #[arg(long = "autotrade-candidate-json")]
        autotrade_candidate_json: Option<String>,
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
                (None, Some(path)) => std::fs::read_to_string(&path).map_err(|e| {
                    anyhow::anyhow!("failed to read --user-content-file {path}: {e}")
                })?,
                (None, None) => bail!("either --user-content or --user-content-file is required"),
            };
            handle_request(
                job_id,
                role,
                agent_id,
                to_agent_id,
                resolved_content,
                list_label,
                llm_content,
                source_event,
            )
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
            template_vars_b64,
        } => {
            let resolved_content = match (user_content, user_content_file) {
                (Some(c), _) => c,
                (None, Some(path)) => std::fs::read_to_string(&path).map_err(|e| {
                    anyhow::anyhow!("failed to read --user-content-file {path}: {e}")
                })?,
                (None, None) => bail!("either --user-content or --user-content-file is required"),
            };
            handle_request_prompt(
                job_id,
                role,
                agent_id,
                to_agent_id,
                resolved_content,
                list_label,
                llm_content,
                source_event,
                template_vars_b64,
            )
        }
        PendingDecisionsV2Command::Resolve { user_reply } => handle_resolve(user_reply),
        PendingDecisionsV2Command::ResolveWithSessionkey {
            user_reply,
            job_id,
            role,
            agent_id,
            to_agent_id,
            source_event,
            autotrade_candidate_json,
        } => handle_resolve_with_sessionkey(
            user_reply,
            job_id,
            role,
            agent_id,
            to_agent_id,
            source_event,
            autotrade_candidate_json,
        ),
        PendingDecisionsV2Command::ResolvePrompt {
            user_reply,
            job_id,
            role,
            agent_id,
            to_agent_id,
            source_event,
            autotrade_candidate_json,
        } => handle_resolve_prompt(
            user_reply,
            job_id,
            role,
            agent_id,
            to_agent_id,
            source_event,
            autotrade_candidate_json,
        ),
        PendingDecisionsV2Command::Pick { index } => handle_pick(index),
        PendingDecisionsV2Command::List { format } => handle_list(format),
        PendingDecisionsV2Command::Cancel { index } => handle_cancel(index),
    }
}

// ─── Handlers ──────────────────────────────────────────────────────────

/// Synchronous direct-push variant of `Request`.
///
/// Branches on `is_cli_mode()`:
/// - CLI driver mode (Claude Code / Codex) → no queue write, no playbook
///   emission; immediately invokes `okx-a2a user decision-request` from inside
///   the CLI. On return the card is already in the user session.
/// - Otherwise (queue mode) → falls back to the same queue-write + playbook
///   emission path as `handle_request`. The LLM still executes the printed
///   `pending-decisions-v2 request` bash block, but the queue lifecycle
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
    template_vars_b64: Option<String>,
) -> Result<()> {
    request_prompt_inner(
        job_id,
        role,
        agent_id,
        to_agent_id,
        user_content,
        list_label,
        llm_content,
        source_event,
        template_vars_b64,
        true,
    )
}

/// In-process direct push for CLI code that must guarantee a decision card
/// reaches the user **without an LLM copy-running `decision_command`** — the
/// autotrade consent-replay guidance historically didn't cover the decision
/// outcome, so a replayed signal's follow-up card (plugin-install / over-cap)
/// was silently dropped. Same enqueue+push semantics as
/// `pending-decisions-v2 request`, minus the `OK` stdout line (callers print
/// their own JSON envelope).
pub(crate) fn push_decision_direct(
    job_id: &str,
    role: &str,
    agent_id: &str,
    to_agent_id: Option<&str>,
    user_content: &str,
    list_label: &str,
    source_event: &str,
) -> Result<()> {
    request_prompt_inner(
        job_id.to_string(),
        role.to_string(),
        agent_id.to_string(),
        to_agent_id.map(str::to_string),
        user_content.to_string(),
        list_label.to_string(),
        None,
        Some(source_event.to_string()),
        None,
        false,
    )
}

/// A decision's relay target can never be the issuer themself. LLMs following
/// the re-ask guidance ("with the same --to-agent-id") were observed filling
/// their OWN agentId when the original card had none — the relay then looks up
/// session `my:X:to:X`, which never exists ("No sessions found"), dead-ending
/// the decision. Self-addressed targets are normalized to None (= the job's
/// backup session), which is also what the original card used.
fn sanitize_to_agent(to_agent_id: Option<String>, agent_id: &str) -> Option<String> {
    match to_agent_id {
        Some(t) if t == agent_id => {
            eprintln!(
                "[pending-v2] --to-agent-id {t} equals --agent-id (self-addressed) — ignored; \
                 routing to the job's backup session instead"
            );
            None
        }
        other => other,
    }
}

/// Auto-trade decisions must resume in the session that received the admitted
/// delivery. The provider id comes from CLI-persisted delivery context, not
/// from model output. `None` remains a compatibility fallback only for
/// old/missing contexts.
fn trusted_autotrade_target(
    job_id: &str,
    agent_id: &str,
    source_event: &str,
    supplied: Option<String>,
) -> Option<String> {
    let supplied = sanitize_to_agent(supplied, agent_id);
    if !source_event.starts_with("autotrade_") {
        return supplied;
    }
    crate::commands::agent_commerce::task::common::autotrade::consent::load_pending_delivery_context(job_id)
        .ok()
        .flatten()
        .map(|context| context.provider_agent_id)
        .filter(|provider| !provider.is_empty() && provider != agent_id)
}

fn trusted_autotrade_session_key(job_id: &str, source_event: &str) -> Option<String> {
    if !source_event.starts_with("autotrade_") {
        return None;
    }
    crate::commands::agent_commerce::task::common::autotrade::consent::load_pending_delivery_context(job_id)
        .ok()
        .flatten()
        .and_then(|context| context.origin_session_key)
        .filter(|key| !key.is_empty())
}

fn trusted_autotrade_delivery_id(job_id: &str, source_event: &str) -> Option<String> {
    if !source_event.starts_with("autotrade_") {
        return None;
    }
    crate::commands::agent_commerce::task::common::autotrade::consent::load_pending_delivery_context(job_id)
        .ok()
        .flatten()
        .map(|context| context.delivery_id)
        .filter(|delivery_id| !delivery_id.is_empty())
}

fn send_decision_relay(
    job_id: &str,
    source_event: &str,
    to_agent_id: Option<&str>,
    content: &str,
) -> Result<()> {
    use sha2::{Digest, Sha256};

    if let Some(session_key) = trusted_autotrade_session_key(job_id, source_event) {
        let message_id = format!(
            "autotrade-relay:{}",
            hex::encode(Sha256::digest(format!(
                "{job_id}\0{source_event}\0{content}"
            )))
        );
        // A known origin is authoritative. Falling back to a job/provider
        // lookup can wake a backup session, splitting processing and its UI
        // result across two sessions. Keep the pending decision intact and let
        // the caller retry when the exact delivery session cannot be reached.
        return super::okx_a2a::session_send_exact(&session_key, content, &message_id);
    }
    super::okx_a2a::session_send(job_id, to_agent_id, content)
}

#[allow(clippy::too_many_arguments)]
fn request_prompt_inner(
    job_id: String,
    role: String,
    agent_id: String,
    to_agent_id: Option<String>,
    user_content: String,
    list_label: String,
    llm_content: Option<String>,
    source_event: Option<String>,
    template_vars_b64: Option<String>,
    print_ok: bool,
) -> Result<()> {
    if crate::commands::agent_commerce::task::common::autotrade::is_retired_mode_configuration_decision(
        source_event.as_deref(),
    ) {
        // Backward-compatible no-op for old Skills or binaries that still try
        // to push a retired execution-mode/configuration prompt.
        if print_ok {
            println!("OK");
        }
        return Ok(());
    }
    let to_agent_id = sanitize_to_agent(to_agent_id, &agent_id);
    let user_content = user_content.replace("\\n", "\n");

    // Template-variable substitution (fail-closed / default-deny).
    // Runs AFTER clap parse and BEFORE `is_cli_mode`, any queue/file write, card
    // construction, or `okx_a2a::user_decision_request`: decode + validate the
    // Base64(JSON) payload (empty map when the flag is absent), then run
    // `template_vars::render_all` UNCONDITIONALLY over `--user-content` /
    // `--list-label`. Running the bijection check on every path is what makes the
    // missing-flag case fail closed: if a reserved `{{__OKX_TASK_TITLE__}}` /
    // `{{__OKX_TASK_LABEL_TITLE__}}` placeholder survives with no matching var
    // (flag omitted, or flag present but the var is missing), `render_all` returns
    // `TEMPLATE_VALUE_MISSING` and we abort BEFORE any side effect — a literal
    // placeholder can never be pushed. A supplied var with no placeholder is
    // `TEMPLATE_PLACEHOLDER_MISSING`; a malformed payload is `TEMPLATE_VARS_INVALID`.
    // With no placeholder AND no flag the render is a no-op, so ordinary decision
    // flows keep their exact legacy output. Every failure maps to a
    // stable, value-free `CodedError` (→ `output::error_coded` + exit 1 in main.rs);
    // the message never embeds the decoded title.
    let (user_content, list_label) = {
        use crate::commands::agent_commerce::task::common::template_vars;
        use crate::commands::sink::CodedError;
        let vars = match template_vars_b64.as_deref() {
            Some(b64) => template_vars::decode_and_validate(b64).map_err(|e| {
                CodedError::new(e.code(), Some("template-vars-b64"), e.to_string())
            })?,
            None => std::collections::BTreeMap::new(),
        };
        let rendered = template_vars::render_all(&[&user_content, &list_label], &vars)
            .map_err(|e| CodedError::new(e.code(), Some("template-vars-b64"), e.to_string()))?;
        let mut it = rendered.into_iter();
        let rendered_content = it.next().expect("render_all yields user_content");
        let rendered_label = it.next().expect("render_all yields list_label");
        (rendered_content, rendered_label)
    };
    let cli_mode = is_cli_mode();
    trace_log(&format!(
        "handle_request_prompt {}: job_id={} role={} agent_id={} to_agent_id={:?}",
        if cli_mode { "CLI_MODE" } else { "QUEUE_MODE" },
        job_id,
        role,
        agent_id,
        to_agent_id,
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
        if print_ok {
            println!("OK");
        }
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
        q.entries
            .retain(|e| !entry_matches(e, &job_id, &role, &agent_id, to_ref));
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
        if print_ok {
            println!("OK");
        }
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
    // Ordinary `request` never carries the untrusted-title template payload — the
    // `sub_user_reject` path emits its own `request-prompt` block. Pass `None` so
    // the shared implementation runs the legacy
    // (no-substitution) path for every other decision flow.
    handle_request_prompt(
        job_id,
        role,
        agent_id,
        to_agent_id,
        user_content,
        list_label,
        llm_content,
        source_event,
        None,
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
    autotrade_candidate_json: Option<String>,
) -> Result<()> {
    trace_log(&format!(
        "handle_resolve_with_sessionkey: job_id={} role={} agent_id={} to_agent_id={:?} source_event={} user_reply={:?}",
        job_id, role, agent_id, to_agent_id, source_event, user_reply,
    ));
    // Rescue path for cards already issued with a poisoned self-addressed target:
    // normalizing here means answering such a card again still relays correctly.
    let to_agent_id = trusted_autotrade_target(&job_id, &agent_id, &source_event, to_agent_id);
    let relay_delivery_id = trusted_autotrade_delivery_id(&job_id, &source_event);
    // Deterministic language capture: the verbatim reply is the one place the
    // CLI reliably sees the user's own words — CLI-rendered copy downstream
    // (direct-pushed decision cards, swap self-notify) renders in this language.
    super::user_lang::record_from_user_text(&job_id, &user_reply);
    let (user_reply, autotrade_outcome) = match prepare_foreground_autotrade(
        &job_id,
        &agent_id,
        &source_event,
        autotrade_candidate_json.as_deref(),
    )? {
        Some(ForegroundAutotrade::Awaiting) => return Ok(()),
        Some(ForegroundAutotrade::Relay {
            normalized_reply,
            outcome,
        }) => (normalized_reply, Some(outcome)),
        None => (user_reply, None),
    };
    let relay_source_event = if autotrade_outcome.is_some() {
        crate::commands::agent_commerce::task::common::autotrade::card::CONSENT_SOURCE_EVENT
    } else {
        source_event.as_str()
    };
    let relay_event = format!("user_decision_{relay_source_event}");
    let relay_data_contract = if autotrade_outcome.is_some() {
        "<foreground-validated normalized A/B/C policy>"
    } else {
        "<message.data verbatim>"
    };
    let delivery_contract = relay_delivery_id
        .as_deref()
        .map(|delivery_id| format!(",\"deliveryId\":\"{delivery_id}\""))
        .unwrap_or_default();
    let description = format!(
        "User-decision relay envelope (CLI mode). Call `onchainos agent next-action \
         --role {role} --agentId {agent} \
         --message '{{\"event\":\"{evt}\",\"jobId\":\"{jid}\",\"data\":\"{data_contract}\"{delivery_contract}}}'` \
         to fetch the routing playbook; follow it. \
         ❌ Do NOT call `pending-decisions-v2 resolve` / `pick` / `cancel` — those are \
         user-session-only; the user-session already issued this relay envelope.",
        jid = job_id,
        evt = relay_event,
        role = role,
        agent = agent_id,
        data_contract = relay_data_contract,
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
            "deliveryId": relay_delivery_id,
            "role": role,
            "timestamp": Utc::now().timestamp(),
        }
    });
    let relay_content = serde_json::to_string(&relay_envelope)
        .unwrap_or_else(|_| format!(
            "{{\"agentId\":\"{}\",\"message\":{{\"event\":\"{}\",\"data\":{:?},\"source\":\"system\",\"jobId\":\"{}\",\"role\":\"{}\"}}}}",
            agent_id, relay_event, user_reply, job_id, role,
        ));
    send_decision_relay(
        &job_id,
        &source_event,
        to_agent_id.as_deref(),
        &relay_content,
    )?;
    if let Some(mut outcome) = autotrade_outcome {
        crate::commands::agent_commerce::task::common::autotrade::consent_reply::clear_candidate_draft(
            &job_id,
        );
        crate::commands::agent_commerce::task::common::autotrade::consent::clear_pending_signal(
            &job_id,
        );
        outcome["deliveryResumeQueued"] = serde_json::Value::Bool(true);
        print_foreground_outcome(&outcome);
        print_foreground_persist_guidance(&outcome);
    }
    print!("{}", decision_relay_post_action());
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
    autotrade_candidate_json: Option<String>,
) -> Result<()> {
    trace_log(&format!(
        "handle_resolve_prompt: job_id={} role={} agent_id={} to_agent_id={:?} source_event={} user_reply={:?}",
        job_id, role, agent_id, to_agent_id, source_event, user_reply,
    ));
    // Same self-addressed-target rescue as `handle_resolve_with_sessionkey`.
    let to_agent_id = trusted_autotrade_target(&job_id, &agent_id, &source_event, to_agent_id);
    let relay_delivery_id = trusted_autotrade_delivery_id(&job_id, &source_event);
    // Same deterministic language capture as `handle_resolve_with_sessionkey`.
    super::user_lang::record_from_user_text(&job_id, &user_reply);
    // Remove the current queue entry before applying the candidate. A missing-
    // field or confirmation result pushes its replacement under the same key;
    // removing after that push would delete the new card.
    remove_prompt_entry(&job_id, &role, &agent_id, to_agent_id.as_deref());
    let (user_reply, autotrade_outcome) = match prepare_foreground_autotrade(
        &job_id,
        &agent_id,
        &source_event,
        autotrade_candidate_json.as_deref(),
    )? {
        Some(ForegroundAutotrade::Awaiting) => return Ok(()),
        Some(ForegroundAutotrade::Relay {
            normalized_reply,
            outcome,
        }) => (normalized_reply, Some(outcome)),
        None => (user_reply, None),
    };
    let relay_source_event = if autotrade_outcome.is_some() {
        crate::commands::agent_commerce::task::common::autotrade::card::CONSENT_SOURCE_EVENT
    } else {
        source_event.as_str()
    };
    let relay_event = format!("user_decision_{relay_source_event}");
    let relay_data_contract = if autotrade_outcome.is_some() {
        "<foreground-validated normalized A/B/C policy>"
    } else {
        "<message.data verbatim>"
    };
    let delivery_contract = relay_delivery_id
        .as_deref()
        .map(|delivery_id| format!(",\"deliveryId\":\"{delivery_id}\""))
        .unwrap_or_default();
    let description = format!(
        "User-decision relay envelope (queue-backed prompt mode). Call `onchainos agent next-action \
         --role {role} --agentId {agent} \
         --message '{{\"event\":\"{evt}\",\"jobId\":\"{jid}\",\"data\":\"{data_contract}\"{delivery_contract}}}'` \
         to fetch the routing playbook; follow it. \
         ❌ Do NOT call `pending-decisions-v2 resolve` / `resolve-with-sessionkey` / `resolve-prompt` / `pick` / `cancel` — those are user-session-only; the user-session already issued this relay envelope.",
        jid = job_id, evt = relay_event, role = role, agent = agent_id,
        data_contract = relay_data_contract,
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
            "deliveryId": relay_delivery_id,
            "role": role,
            "timestamp": Utc::now().timestamp(),
        }
    });
    let relay_content = serde_json::to_string(&relay_envelope)
        .unwrap_or_else(|_| format!(
            "{{\"agentId\":\"{}\",\"message\":{{\"event\":\"{}\",\"data\":{:?},\"source\":\"system\",\"jobId\":\"{}\",\"role\":\"{}\"}}}}",
            agent_id, relay_event, user_reply, job_id, role,
        ));

    let to_ref = to_agent_id.as_deref();
    send_decision_relay(&job_id, &source_event, to_ref, &relay_content)?;
    if let Some(mut outcome) = autotrade_outcome {
        crate::commands::agent_commerce::task::common::autotrade::consent_reply::clear_candidate_draft(
            &job_id,
        );
        crate::commands::agent_commerce::task::common::autotrade::consent::clear_pending_signal(
            &job_id,
        );
        outcome["deliveryResumeQueued"] = serde_json::Value::Bool(true);
        print_foreground_outcome(&outcome);
        print_foreground_persist_guidance(&outcome);
    }
    print!("{}", decision_relay_post_action());
    Ok(())
}

enum ForegroundAutotrade {
    Awaiting,
    Relay {
        normalized_reply: String,
        outcome: serde_json::Value,
    },
}

fn prepare_foreground_autotrade(
    job_id: &str,
    agent_id: &str,
    source_event: &str,
    candidate_json: Option<&str>,
) -> Result<Option<ForegroundAutotrade>> {
    use crate::commands::agent_commerce::task::common::autotrade::consent_reply;
    if !consent_reply::is_candidate_source(source_event) {
        if candidate_json.is_some() {
            bail!("--autotrade-candidate-json is only valid for auto-trade consent decisions");
        }
        return Ok(None);
    }
    let Some(candidate_json) = candidate_json else {
        // Backward compatibility for cards created by older binaries: retain
        // the original background relay when no structured candidate exists.
        return Ok(None);
    };
    match consent_reply::apply_candidate_json(job_id, agent_id, source_event, candidate_json)? {
        consent_reply::ApplyResult::FallbackRelay => Ok(None),
        consent_reply::ApplyResult::Awaiting(outcome) => {
            print_foreground_outcome(&outcome);
            println!(
                "The auto-trade draft was processed synchronously. A follow-up decision is already available; end this turn and wait for the user's reply."
            );
            Ok(Some(ForegroundAutotrade::Awaiting))
        }
        consent_reply::ApplyResult::Relay {
            normalized_reply,
            outcome,
        } => Ok(Some(ForegroundAutotrade::Relay {
            normalized_reply,
            outcome,
        })),
    }
}

fn print_foreground_outcome(outcome: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string(outcome).unwrap_or_else(|_| "{}".to_string())
    );
}

fn print_foreground_persist_guidance(outcome: &serde_json::Value) {
    if outcome
        .get("authorizationPersisted")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        println!(
            "Authorization was written synchronously before delivery resume. Do not describe it as still processing."
        );
    } else {
        println!(
            "The skip decision was applied synchronously and no authorization was written. Do not describe it as still processing."
        );
    }
}

fn remove_prompt_entry(job_id: &str, role: &str, agent_id: &str, to_agent_id: Option<&str>) {
    // Best-effort queue cleanup. The in-process candidate handling / relay is
    // the critical path and must still run when this local cleanup fails.
    match acquire_lock() {
        Ok(_lock) => match read_queue() {
            Ok(mut q) => {
                let before = q.entries.len();
                q.entries
                    .retain(|e| !entry_matches(e, job_id, role, agent_id, to_agent_id));
                if q.entries.len() != before {
                    if let Err(e) = write_queue_atomic(&q) {
                        trace_log(&format!(
                            "handle_resolve_prompt: write_queue_atomic failed: {e}"
                        ));
                    }
                }
            }
            Err(e) => trace_log(&format!("handle_resolve_prompt: read_queue failed: {e}")),
        },
        Err(e) => trace_log(&format!("handle_resolve_prompt: acquire_lock failed: {e}")),
    }
}

fn handle_resolve(user_reply: String) -> Result<()> {
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
    let source_event = active.source_event.as_deref().unwrap_or("");
    let relay_delivery_id =
        trusted_autotrade_delivery_id(&active.job_id, source_event);
    let clear_pending_after_relay = source_event.starts_with("autotrade_");
    // Same deterministic language capture as the sessionkey/prompt resolve variants.
    super::user_lang::record_from_user_text(&active.job_id, &user_reply);
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
    let delivery_contract = relay_delivery_id
        .as_deref()
        .map(|delivery_id| format!(",\"deliveryId\":\"{delivery_id}\""))
        .unwrap_or_default();
    let description = format!(
        "User-decision relay envelope (sub session). Call `onchainos agent next-action \
         --role {role} --agentId {agent} \
         --message '{{\"event\":\"{evt}\",\"jobId\":\"{jid}\",\"data\":\"<message.data verbatim>\"{delivery_contract}}}'` \
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
            "deliveryId": relay_delivery_id,
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
        send_decision_relay(
            &active.job_id,
            source_event,
            active.to_agent_id.as_deref(),
            &relay_content,
        )?;
        if clear_pending_after_relay {
            crate::commands::agent_commerce::task::common::autotrade::consent::clear_pending_signal(
                &active.job_id,
            );
        }
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
            .position(|e| {
                entry_matches(
                    e,
                    &promote.job_id,
                    &promote.role,
                    &promote.agent_id,
                    promote_to_ref,
                )
            })
            .unwrap();
        q.entries[promote_idx].status = Status::Active;
        // Re-sort so the newly-promoted active sits at index 0 (the sort honors the
        // "active first, then LIFO" invariant).
        ensure_invariant_and_evict(&mut q);

        // Relay before consuming the active queue entry on disk. If exact
        // delivery-session delivery fails, the user's decision remains
        // recoverable and can be retried.
        send_decision_relay(
            &active.job_id,
            source_event,
            active.to_agent_id.as_deref(),
            &relay_content,
        )?;
        if clear_pending_after_relay {
            crate::commands::agent_commerce::task::common::autotrade::consent::clear_pending_signal(
                &active.job_id,
            );
        }
        let snap = build_snapshot(&q);
        write_snapshot_atomic(&snap)?;
        write_queue_atomic(&q)?;
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
        print!(
            "{}",
            playbook_stale_relist(&new_snap, "selection index out of range")
        );
        return Ok(());
    }

    let target = snapshot.items[index - 1].clone();
    let target_to = target.to_agent_id.as_deref();
    let snap_displayed_at = snapshot.displayed_at;

    let entry_idx = q
        .entries
        .iter()
        .position(|e| entry_matches(e, &target.job_id, &target.role, &target.agent_id, target_to));
    let Some(entry_idx) = entry_idx else {
        let new_snap = build_snapshot(&q);
        write_snapshot_atomic(&new_snap)?;
        print!(
            "{}",
            playbook_stale_relist(
                &new_snap,
                "selected entry no longer exists (auto-cleaned or resolved)"
            )
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
                playbook_stale_relist(
                    &new_snap,
                    "selected entry's content was updated since display"
                )
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
    let Some(entry_idx) = q
        .entries
        .iter()
        .position(|e| entry_matches(e, &target.job_id, &target.role, &target.agent_id, target_to))
    else {
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
            .map(|e| {
                (
                    e.job_id.clone(),
                    e.role.clone(),
                    e.agent_id.clone(),
                    e.to_agent_id.clone(),
                )
            });
        if let Some((j, r, a, t)) = newest_queued_key {
            if let Some(promote_idx) = q
                .entries
                .iter()
                .position(|e| entry_matches(e, &j, &r, &a, t.as_deref()))
            {
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
/// (`pending-decisions-v2 request` / `onchainos agent user-notify` / end-turn) and explicitly
/// warns that stdout IS the next-action playbook (not log output). Without this, smaller models
/// tend to stop after the bash call — the user-facing tool invocation never happens,
/// the card never surfaces, the flow stalls (24h auto-refund / mistaken auto-decline).
///
/// Arguments:
/// - `job_id`: full hex jobId
/// - `role`: `user` | `provider` | `evaluator`
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
    let user_content_escaped = user_content.replace('\\', "\\\\").replace('"', "\\\"");
    let to_flag = match to_agent_id {
        Some(t) => format!(" --to-agent-id \"{t}\""),
        None => String::new(),
    };
    format!(
        "**Localize first** — translate the `--user-content` and `--list-label` values below to the user's language before running. Keep the bash structure / flags / source-event token unchanged.\n\n\
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

/// Encode the raw (untrusted) decision-copy title and list-label title as the
/// Base64(JSON) value for `--template-vars-b64`. This is the ONLY place
/// the raw titles touch an emitted command, and they emerge as standard-charset
/// Base64 (shell-safe) only. The two keys stay independent so the base
/// title-source precedence (copy = `jobTitle`→`title`→`title_display`; label =
/// `title_display`) is preserved rather than collapsed into one value. `serde_json`
/// guarantees the strings are JSON-escaped; the payload round-trips through
/// [`template_vars::decode_and_validate`](super::template_vars::decode_and_validate).
pub fn encode_title_vars(copy_title: &str, label_title: &str) -> String {
    use base64::Engine;
    let obj = serde_json::json!({
        "__OKX_TASK_TITLE__": copy_title,
        "__OKX_TASK_LABEL_TITLE__": label_title,
    });
    base64::engine::general_purpose::STANDARD
        .encode(serde_json::to_vec(&obj).expect("json object serializes"))
}

/// Map internal role enum to the short user-facing label used in notifications.
fn role_short_label(role: &str) -> &str {
    match role {
        "user" => "User",
        "asp" => "ASP",
        "evaluator" => "Evaluator",
        other => other,
    }
}

/// CLI-driver variant of `resolve_llm_content`. The queue file is bypassed in
/// CLI mode, so the future `resolve` call cannot reverse-lookup routing fields
/// from a queue entry — embed all of them up front so the LLM passes them
/// verbatim to `resolve-with-sessionkey`.
fn foreground_autotrade_candidate_guidance(source_event: &str) -> &'static str {
    if crate::commands::agent_commerce::task::common::autotrade::is_retired_mode_configuration_decision(
        Some(source_event),
    ) {
        return "";
    }
    if !crate::commands::agent_commerce::task::common::autotrade::consent_reply::is_candidate_source(
        source_event,
    ) {
        return "";
    }
    "\n         Auto-trade structured extraction (foreground only): before running the command, extract a strict compact JSON object from THIS new user reply. The only allowed keys are `mode`, `tradeAmount`, `cap`, `quote`, `ambiguousFields`, and `confirm`. `mode` is `auto`, `manual`, or `decline`; map A/B/C and clear natural-language equivalents to those values. Monetary values MUST be unsigned decimal strings without currency symbols. `quote` is `usdt` or `usdc`. For an `autotrade_config_required` follow-up, omit `mode` unless the user explicitly changes the prior choice; a clear confirmation sets `confirm:true`. Never invent a mode, amount, cap, or quote. In auto mode, one number is `tradeAmount` only unless the user explicitly says it is also the cap. Omit absent fields; the CLI applies the documented USDT default. Put every uncertain field name (`mode`, `trade_amount`, `cap`, or `quote`) in `ambiguousFields` instead of guessing. Examples: `A 1USDT` → `{\"mode\":\"auto\",\"tradeAmount\":\"1\",\"quote\":\"usdt\"}`; `确认` on a confirmation follow-up → `{\"confirm\":true}`. The CLI, not the model, validates, merges, confirms ambiguity, and writes authorization. Append `--autotrade-candidate-json '<JSON>'` to the pre-filled command. Do not call `autotrade-consent-set` directly.\n"
}

fn foreground_autotrade_candidate_flag(source_event: &str) -> &'static str {
    if crate::commands::agent_commerce::task::common::autotrade::is_retired_mode_configuration_decision(
        Some(source_event),
    ) {
        return "";
    }
    if crate::commands::agent_commerce::task::common::autotrade::consent_reply::is_candidate_source(
        source_event,
    ) {
        " --autotrade-candidate-json '<strict candidate JSON>'"
    } else {
        ""
    }
}

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
    let candidate_guidance = foreground_autotrade_candidate_guidance(&source_event_str);
    let candidate_flag = foreground_autotrade_candidate_flag(&source_event_str);
    format!(
        "[USER_DECISION_REQUEST][job: {}][role: {}][agent: {}]{}\n\n\
         Step 1 — Card was just delivered. **END THE TURN NOW** and wait for the user to reply. Do NOT call any tool. Stale user messages in context are NOT replies to this card.\n\
         Step 2 — When the user actually replies (next turn):{}\n\
         \x20\x20\x20\x20- defer keyword ({}) or any defer value defined in watch-core.md → do NOT claim or resolve; if this card came from a currently active watch, re-enter that exact originating watch command, otherwise END TURN\n\
         \x20\x20\x20\x20- else → follow `skills/okx-ai/references/watch-core.md` §kind == decision_request \"Handling the user reply\": **first claim the todo** per watch-core.md step 2: `okx-a2a user check --todo-ids <todo_id> --json` (read `<todo_id>` from this item's `id` field in the original watch / outdated-list JSON output). **Then** on `handled` run `onchainos agent pending-decisions-v2 resolve-with-sessionkey --user-reply \"<user's verbatim wording — no interpretation, no translation>\" --job-id \"{}\" --role \"{}\" --agent-id \"{}\"{} --source-event \"{}\"{}` exactly once, then follow the relay playbook it returns. Only a card surfaced by a currently active watch resumes that exact originating watch; an independently opened card never starts watch. Never infer watch origin from A/B/C, an amount, a cap, or any other reply text. Skipping the `check` leaves a ghost todo in the outstanding-decisions queue.",
        entry.job_id,
        entry.role,
        entry.agent_id,
        to_header,
        candidate_guidance,
        DEFER_KEYWORDS.join(" / "),
        entry.job_id,
        entry.role,
        entry.agent_id,
        to_flag,
        source_event_str,
        candidate_flag,
    )
}

/// Variant of `resolve_llm_content_cli` for the `playbook_push_prompt_user`
/// (queue-mode, i.e. `!is_cli_mode()`) path. Adds a multi-decision disambiguation branch in
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
    let candidate_guidance = foreground_autotrade_candidate_guidance(&source_event_str);
    let candidate_flag = foreground_autotrade_candidate_flag(&source_event_str);
    format!(
        "[USER_DECISION_REQUEST]\n\
         [job: {job}][role: {role}][agent: {agent}]{to_header}\n\
         (Anything above this marker is stale — NOT a reply to this card.)\n\n\
         Step 1 — Card was already delivered to the user. You MUST NOT re-render it, paraphrase it, summarize it, translate it again, or compose your own \"please choose A/B/...\" prompt — the user already has the exact text. Stale user messages in context are NOT replies to this card.\n\
         Step 2 — Scan your current context for OTHER [USER_DECISION_REQUEST] blocks. \
         If you find any, render the warning below to the user as your assistant response (in user's language), e.g.:\n\
         \x20\x20`⚠️ You have multiple decisions pending — please prefix your reply with the jobId short hash, e.g. \\`0x7091: approve\\`, so it routes correctly.`\n\
         If no other blocks → skip this step.\n\n\
         Step 3 — **END THE TURN NOW with NO assistant text output** (unless Step 2 fired its multi-card warning, which is the ONLY allowed text this turn). No confirmation, no recap, no fabricated option list. Just stop. Wait for the user to reply in a future turn.\n\n\
         🛑 **The block below runs ONLY in a future turn**, AFTER the user has actually replied. Do NOT run anything in the current turn.\n\
         On the user's next reply, re-scan your context for [USER_DECISION_REQUEST] blocks (the count may have changed since Step 2), then walk this decision tree:{candidate_guidance}\n\
         \x20\x20- defer keyword ({defer}) or any defer value defined in watch-core.md → do NOT claim or resolve; if this card came from a currently active watch, re-enter that exact originating watch command, otherwise END TURN.\n\
         \x20\x20· Reply starts with `0x...:` prefix → strip the prefix + colon, use the prefix to match each block's `[job: 0x...]` header, locate THAT block, then run THAT block's command template with `--user-reply` set to the stripped wording (without the prefix).\n\
         \x20\x20· No prefix + only THIS block in context (single) → run THIS block's command template with the full reply.\n\
         \x20\x20· 🔁 No prefix + **multiple** [USER_DECISION_REQUEST] blocks in context → user forgot to add the jobId prefix. Ask them which jobId they're answering (number the candidates `1. Job 0x...`, `2. Job 0x...`, one per line — short_jobId only), **END THE TURN**, wait for the pick (hex prefix `0x7091` or list number `1`); locate THAT block via `[job: 0x...]` header (or list order), then run THAT block's command template. Never guess, never collapse.\n\n\
         **Command template** (pre-filled for THIS block; only run AFTER the user has replied):\n\
         \x20\x20`onchainos agent pending-decisions-v2 resolve-prompt --user-reply \"<user wording, without any jobId prefix>\" --job-id \"{job}\" --role \"{role}\" --agent-id \"{agent}\"{to_flag} --source-event \"{src}\"{candidate_flag}`\n\n\
         After running, follow the relay playbook the command returns.",
        job = entry.job_id,
        role = entry.role,
        agent = entry.agent_id,
        to_header = to_header,
        to_flag = to_flag,
        src = source_event_str,
        defer = DEFER_KEYWORDS.join(" / "),
        candidate_guidance = candidate_guidance,
        candidate_flag = candidate_flag,
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
    // body to match the user's language before `onchainos agent user-notify`. We do NOT
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
         **Step 2 — Run `onchainos agent user-notify` with the localized content from Step 1**:\n\
         ```bash\n\
         onchainos agent user-notify --content \"<the localized Step 1 output>\"\n\
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
         **User-visible text** (render this verbatim as your assistant response; translate per [Localization] rules if the user's language is not English; keep `jobId` / data values intact):\n\
         \"\"\"\n{}\"\"\"\n\n\
         **LLM context** (this is for YOUR own routing reasoning — **do NOT show / paraphrase / leak this block to the user**):\n\
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
        list.push_str(&format!(
            "\nReply with a number 1-{} to re-select.\n",
            snap.items.len()
        ));
    }
    format!(
        "The previous selection is stale. **Translate the content below into the user's language**, then render as your assistant response:\n\n\
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
#[cfg(test)]
mod template_var_emitter_tests {
    use super::encode_title_vars;
    use crate::commands::agent_commerce::task::common::template_vars;

    // encode_title_vars is the shared emitter primitive used by the retained
    // `sub_user_reject` renderer; both keys must round-trip through the decode
    // path so the pushed card body equals the original titles. (The ordinary
    // `request_command_block` no longer takes a template payload — the raw-title
    // → request-prompt output is exercised by the asp/flow.rs `sub_user_reject`
    // renderer tests and the cli/tests shell-injection integration test.)
    #[test]
    fn encode_title_vars_round_trips_through_decode() {
        for title in [
            "Weekly Report",
            // CJK title (U+4E2D U+6587 U+6807 U+9898) + rocket emoji as \u escapes:
            // no raw CJK bytes in source, identical String at runtime.
            "\u{4e2d}\u{6587}\u{6807}\u{9898}\u{1f680}",
            "Oli's task",
            "`id`",
            "$(touch /tmp/x)",
            "\"; id; #",
        ] {
            // copy != label to prove the two keys stay independent.
            let label = format!("<label>{title}");
            let b64 = encode_title_vars(title, &label);
            let vars = template_vars::decode_and_validate(&b64).expect("valid payload");
            assert_eq!(
                vars.get("__OKX_TASK_TITLE__").map(String::as_str),
                Some(title)
            );
            assert_eq!(
                vars.get("__OKX_TASK_LABEL_TITLE__").map(String::as_str),
                Some(label.as_str())
            );
        }
    }
}

#[cfg(test)]
mod request_prompt_fail_closed_tests {
    use super::{encode_title_vars, request_prompt_inner};
    use crate::commands::agent_commerce::task::common::template_vars;
    use crate::commands::sink::CodedError;

    // Reserved placeholders exercised across the four field positions. The
    // untrusted title never appears in any of these constants — only the fixed
    // placeholders and value-free copy.
    const COPY_PH: &str = "Body: {{__OKX_TASK_TITLE__}} — review.";
    const LABEL_PH: &str = "[Decision 0xjob] {{__OKX_TASK_LABEL_TITLE__}} decision";
    const PLAIN: &str = "no reserved placeholder here";

    // Drive `request_prompt_inner` exactly as the real `sub_user_reject` emitter
    // does (`print_ok=false`, `source_event=Some`), with an explicit flag choice.
    // Returns the downcast `CodedError` — obtaining one is itself proof that the
    // call aborted BEFORE any side effect: the substitution stage runs before
    // `is_cli_mode`, before every queue/file write, and before
    // `okx_a2a::user_decision_request` (the only card-push site), and the push path
    // can never surface a `TEMPLATE_*` code. So a `TEMPLATE_*` CodedError ⇒ the
    // fake `okx-a2a` was invoked zero times and no queue/card/file was written.
    fn fail_closed(user_content: &str, list_label: &str, flag: Option<&str>) -> CodedError {
        let err = request_prompt_inner(
            "0xjob".to_string(),
            "asp".to_string(),
            "42".to_string(),
            None,
            user_content.to_string(),
            list_label.to_string(),
            None,
            Some("sub_user_reject".to_string()),
            flag.map(str::to_string),
            false,
        )
        .expect_err("fail-closed: request_prompt_inner must return Err before any push");
        let coded = err
            .downcast_ref::<CodedError>()
            .expect("fail-closed error must downcast to CodedError")
            .clone();
        assert_eq!(coded.field.as_deref(), Some("template-vars-b64"));
        // The surfaced message is value-free — it never embeds a
        // variable name or a decoded title fragment.
        assert!(
            !coded.message.contains("__OKX")
                && !coded.message.contains("TITLE")
                && !coded.message.contains("review"),
            "fail-closed message must not leak variable internals or title data: {}",
            coded.message
        );
        coded
    }

    // ── Missing flag must fail closed BEFORE every side effect ────────────────

    // Copy placeholder + no flag → TEMPLATE_VALUE_MISSING, no push.
    #[test]
    fn copy_placeholder_no_flag_value_missing() {
        assert_eq!(
            fail_closed(COPY_PH, "plain label", None).code,
            template_vars::CODE_VALUE_MISSING
        );
    }

    // List-label placeholder + no flag → TEMPLATE_VALUE_MISSING, no push.
    #[test]
    fn label_placeholder_no_flag_value_missing() {
        assert_eq!(
            fail_closed("plain content", LABEL_PH, None).code,
            template_vars::CODE_VALUE_MISSING
        );
    }

    // Both placeholders + no flag → TEMPLATE_VALUE_MISSING, no push.
    #[test]
    fn both_placeholders_no_flag_value_missing() {
        assert_eq!(
            fail_closed(COPY_PH, LABEL_PH, None).code,
            template_vars::CODE_VALUE_MISSING
        );
    }

    // Flag present but its var has no matching placeholder → TEMPLATE_PLACEHOLDER_MISSING.
    #[test]
    fn flag_with_missing_placeholder_placeholder_missing() {
        let b64 = encode_title_vars("X", "Y"); // supplies both whitelisted vars
        assert_eq!(
            fail_closed(PLAIN, "still none", Some(&b64)).code,
            template_vars::CODE_PLACEHOLDER_MISSING
        );
    }

    // Malformed Base64 payload → TEMPLATE_VARS_INVALID, no push.
    #[test]
    fn bad_base64_fails_closed_without_pushing_a_card() {
        assert_eq!(
            fail_closed(COPY_PH, LABEL_PH, Some("not*valid*base64!!!")).code,
            template_vars::CODE_VARS_INVALID
        );
    }

    // Valid-but-empty payload (`{}`, Base64 `e30=`) against a declared placeholder
    // → TEMPLATE_VALUE_MISSING, no push.
    #[test]
    fn empty_payload_with_placeholder_fails_closed() {
        assert_eq!(
            fail_closed(COPY_PH, "plain label", Some("e30=")).code, // base64("{}")
            template_vars::CODE_VALUE_MISSING
        );
    }
}

// Shell-safety proof for the shared encoding, scoped to the retained
// `sub_user_reject` site. A real zsh/Bash process-spawn harness is
// deliberately NOT used here: the security invariant is that the dangerous bytes
// are provably absent from the emitted command line, so a shell that later runs
// the block is a no-op w.r.t. injection — and the title reaches the `okx-a2a`
// binary only as an argv element of `Command::new(...).args(...)` (never `sh -c`),
// which this decode+render round-trip reproduces exactly. (Sites 1/3/4/5 were
// not covered by this unit-test module.)
#[cfg(test)]
mod shared_encoding_shell_safety_tests {
    use super::encode_title_vars;
    use crate::commands::agent_commerce::task::common::template_vars::{
        self, decode_and_validate, render_all,
    };

    // Malicious corpus: backtick + $(...) command substitution, quote/`;`/`#`
    // break-out, embedded newline, CJK/emoji, an apostrophe title, and a title that
    // is literally a placeholder (single-pass / non-recursive edge case).
    const CORPUS: &[&str] = &[
        // A hostile payload using zsh arithmetic/`(e)`
        // expansion that reconstructs `id>&2` from `${(#):-96}` = '`'). If any byte
        // of this reached a zsh command line it would execute `id`; the placeholder
        // + Base64 mechanism must keep it entirely off the emitted line.
        "x${(e):-${(#):-96}id>&2${(#):-96}}",
        "`id`",
        "$(touch /tmp/oli_sentinel)",
        "\"; id; #",
        "line1\nline2",
        // CJK title (U+4E2D U+6587 U+6807 U+9898) + rocket emoji as \u escapes:
        // no raw CJK bytes in source, identical String at runtime.
        "\u{4e2d}\u{6587}\u{6807}\u{9898}\u{1f680}",
        "Oli's task",
        "{{__OKX_TASK_TITLE__}}",
        "plain title",
    ];

    #[test]
    fn shared_encoding_keeps_titles_off_the_command_line_and_round_trips() {
        // sub_user_reject carries two independent titles: decision-copy + list-label.
        let content = "Body copy: {{__OKX_TASK_TITLE__}} — please review.";
        let label = "[Decision 0xabc] {{__OKX_TASK_LABEL_TITLE__}} — refund or dispute";
        for &copy_title in CORPUS {
            for &label_title in CORPUS {
                let b64 = encode_title_vars(copy_title, label_title);

                // (1) command-line safety: the shell-safe Base64 payload never
                // contains a raw title byte. The literal-placeholder corpus members
                // are skipped (they legitimately appear as the substitution target).
                for raw in [copy_title, label_title] {
                    if raw != template_vars::TITLE_PLACEHOLDER
                        && raw != template_vars::LABEL_TITLE_PLACEHOLDER
                    {
                        assert!(
                            !b64.contains(raw),
                            "raw title {raw:?} must be off the emitted line (Base64 only)"
                        );
                    }
                }

                // (2)+(3) the CLI's own decode + single-pass render (what
                // request_prompt_inner runs after clap parse, before any push)
                // reconstructs the EXACT original titles, non-recursively, each key
                // independent of the other.
                let vars = decode_and_validate(&b64).expect("emitted payload decodes");
                let rendered = render_all(&[content, label], &vars).expect("renders");
                assert_eq!(
                    rendered[0],
                    content.replace(template_vars::TITLE_PLACEHOLDER, copy_title),
                    "copy renders to the exact title (single-pass) for {copy_title:?}"
                );
                assert_eq!(
                    rendered[1],
                    label.replace(template_vars::LABEL_TITLE_PLACEHOLDER, label_title),
                    "label renders to the exact title (single-pass) for {label_title:?}"
                );
            }
        }
    }
}

#[cfg(test)]
mod sanitize_tests {
    use super::{
        decision_relay_post_action, read_queue, resolve_llm_content_cli,
        resolve_llm_content_prompt_user, request_prompt_inner, sanitize_to_agent,
        trusted_autotrade_session_key, write_queue_atomic, PendingEntry, Queue, Status,
    };
    use chrono::Utc;

    fn decision_entry() -> PendingEntry {
        let now = Utc::now();
        PendingEntry {
            job_id: "job-123".to_string(),
            role: "user".to_string(),
            agent_id: "8315".to_string(),
            to_agent_id: None,
            user_content: "Choose A/B/C".to_string(),
            list_label: "decision".to_string(),
            llm_content_override: None,
            source_event: Some("autotrade_consent".to_string()),
            status: Status::Active,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn self_addressed_target_normalizes_to_backup() {
        // The 8315→8315 "No sessions found" bug: an LLM re-ask filled its own
        // agentId as the counterparty. Self must normalize to None (backup).
        assert_eq!(sanitize_to_agent(Some("8315".into()), "8315"), None);
        // Real counterparty passes through; None stays None.
        assert_eq!(
            sanitize_to_agent(Some("4941".into()), "8315"),
            Some("4941".into())
        );
        assert_eq!(sanitize_to_agent(None, "8315"), None);
    }

    #[test]
    fn defer_guidance_delegates_to_watch_core_contract() {
        for content in [
            resolve_llm_content_cli(&decision_entry()),
            resolve_llm_content_prompt_user(&decision_entry()),
        ] {
            assert!(content.contains("any defer value defined in watch-core.md"));
        }
    }

    #[test]
    fn both_default_decision_modes_preserve_the_watch_origin_guard() {
        for content in [
            resolve_llm_content_cli(&decision_entry()),
            resolve_llm_content_prompt_user(&decision_entry()),
        ] {
            assert!(content.contains("currently active watch"));
            assert!(content.contains("exact originating watch command"));
            assert!(content.contains("otherwise END TURN"));
        }
    }

    #[test]
    fn post_relay_guidance_uses_origin_not_reply_text() {
        let guidance = decision_relay_post_action();
        assert!(guidance.contains("currently active `okx-a2a user watch`"));
        assert!(guidance.contains("preserve global vs sticky `--job-id`"));
        assert!(guidance.contains("decision list / outdated-list"));
        assert!(guidance.contains("Never infer watch origin from the user's reply text"));
    }

    #[test]
    fn retired_policy_decisions_never_request_candidate_extraction() {
        for source in ["autotrade_consent", "autotrade_config_required"] {
            let mut entry = decision_entry();
            entry.source_event = Some(source.to_string());
            for content in [
                resolve_llm_content_cli(&entry),
                resolve_llm_content_prompt_user(&entry),
            ] {
                assert!(!content.contains("--autotrade-candidate-json"));
                assert!(!content.contains("strict compact JSON"));
            }
        }
    }

    #[test]
    fn non_autotrade_decisions_keep_the_generic_resolver_contract() {
        let mut entry = decision_entry();
        entry.source_event = Some("job_submitted".to_string());
        for content in [
            resolve_llm_content_cli(&entry),
            resolve_llm_content_prompt_user(&entry),
        ] {
            assert!(!content.contains("--autotrade-candidate-json"));
            assert!(!content.contains("strict compact JSON"));
        }
    }

    #[test]
    fn legacy_autotrade_mode_cards_are_absorbed_from_the_local_queue() {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("pending-v2-retired-mode-test");
        std::fs::create_dir_all(&root).unwrap();
        let home = tempfile::tempdir_in(root).unwrap();
        std::env::set_var("ONCHAINOS_HOME", home.path());

        let retired = decision_entry();
        let mut retired_config = decision_entry();
        retired_config.job_id = "job-config".to_string();
        retired_config.source_event = Some("autotrade_config_required".to_string());
        let mut regular = decision_entry();
        regular.job_id = "job-regular".to_string();
        regular.source_event = Some("job_submitted".to_string());
        write_queue_atomic(&Queue {
            entries: vec![retired, retired_config, regular],
        })
        .unwrap();

        let loaded = read_queue().unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].job_id, "job-regular");

        request_prompt_inner(
            "job-new".to_string(),
            "user".to_string(),
            "8315".to_string(),
            None,
            "retired mode card".to_string(),
            "retired".to_string(),
            None,
            Some("autotrade_consent".to_string()),
            false,
        )
        .unwrap();
        request_prompt_inner(
            "job-config-new".to_string(),
            "user".to_string(),
            "8315".to_string(),
            None,
            "retired configuration card".to_string(),
            "retired".to_string(),
            None,
            Some("autotrade_config_required".to_string()),
            false,
        )
        .unwrap();
        let loaded_after_request = read_queue().unwrap();
        assert_eq!(loaded_after_request.entries.len(), 1);
        assert_eq!(loaded_after_request.entries[0].job_id, "job-regular");
        std::env::remove_var("ONCHAINOS_HOME");
    }

    #[test]
    fn autotrade_route_prefers_persisted_origin_session_key() {
        let _lock = crate::home::TEST_ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("pending-v2-origin-session-test");
        std::fs::create_dir_all(&root).unwrap();
        let home = tempfile::tempdir_in(root).unwrap();
        std::env::set_var("ONCHAINOS_HOME", home.path());
        let context = crate::commands::agent_commerce::task::common::autotrade::consent::register_delivery_context(
            "job1",
            "8315",
            "8779",
            Some("job:job1:my:8315:to:8779"),
            "delivery-1",
            "/tmp/signal.txt",
            "text",
            1,
        )
        .unwrap();
        crate::commands::agent_commerce::task::common::autotrade::consent::activate_delivery_context(
            &context.job_id,
            &context.delivery_id,
        )
        .unwrap();
        assert_eq!(
            trusted_autotrade_session_key("job1", "autotrade_consent").as_deref(),
            Some("job:job1:my:8315:to:8779")
        );
        assert_eq!(trusted_autotrade_session_key("job1", "job_submitted"), None);
        std::env::remove_var("ONCHAINOS_HOME");
    }
}
