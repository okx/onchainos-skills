//! Pending-decisions v2 — redesigned queue with single-active invariant,
//! implicit state machine, sessionKey primary key, and LLM-playbook output.
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
    sub_key: String,
    job_id: String,
    role: String,
    agent_id: String,
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
    sub_key: String,
    list_label: String,
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
    /// (sub) Enqueue a new user-decision request. Overwrites same sub_key.
    Request {
        #[arg(long = "sub-key")]
        sub_key: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        role: String,
        #[arg(long = "agent-id")]
        agent_id: String,
        /// Full user-facing text (verbatim rendered to chat).
        #[arg(long = "user-content")]
        user_content: String,
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

    /// (user-session) Resolve the current active decision with user's reply.
    Resolve {
        #[arg(long = "user-reply")]
        user_reply: String,
    },

    /// (user-session, CLI-driver bypass) Resolve a decision without consulting
    /// the queue file — caller passes every routing field explicitly so the
    /// envelope can be built and dispatched. Pairs with `request`'s
    /// OKX_A2A_IS_CLI=1 bypass; used when a non-MCP CLI loop owns turn-taking
    /// and never persists queue state to disk.
    #[command(name = "resolve-with-sessionkey")]
    ResolveWithSessionkey {
        #[arg(long = "user-reply")]
        user_reply: String,
        #[arg(long = "sub-key")]
        sub_key: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        role: String,
        #[arg(long = "agent-id")]
        agent_id: String,
        #[arg(long = "source-event")]
        source_event: String,
    },

    /// (user-session, MCP variant of resolve-with-sessionkey) Same envelope
    /// construction as `resolve-with-sessionkey`, but emits a playbook that
    /// dispatches via the MCP `xmtp_dispatch_session` tool instead of the
    /// `okx-a2a session send` CLI subprocess. Pairs with `playbook_push_prompt_user`
    /// (the MCP push variant), so an MCP push → MCP relay round-trip stays consistent.
    #[command(name = "resolve-prompt")]
    ResolvePrompt {
        #[arg(long = "user-reply")]
        user_reply: String,
        #[arg(long = "sub-key")]
        sub_key: String,
        #[arg(long = "job-id")]
        job_id: String,
        #[arg(long)]
        role: String,
        #[arg(long = "agent-id")]
        agent_id: String,
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
    /// Pass exactly one of --sub-key or --index to identify the target.
    /// If the cancelled entry was Active, the newest Queued entry is auto-promoted (LIFO).
    Cancel {
        /// Cancel by full XMTP sessionKey (precise).
        #[arg(long = "sub-key", conflicts_with = "index")]
        sub_key: Option<String>,
        /// Cancel by 1-based index from the latest `list` / snapshot.
        #[arg(long, conflicts_with = "sub_key")]
        index: Option<usize>,
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
            sub_key,
            job_id,
            role,
            agent_id,
            user_content,
            list_label,
            llm_content,
            source_event,
        } => handle_request(sub_key, job_id, role, agent_id, user_content, list_label, llm_content, source_event),
        PendingDecisionsV2Command::Resolve { user_reply } => handle_resolve(user_reply),
        PendingDecisionsV2Command::ResolveWithSessionkey {
            user_reply, sub_key, job_id, role, agent_id, source_event,
        } => handle_resolve_with_sessionkey(user_reply, sub_key, job_id, role, agent_id, source_event),
        PendingDecisionsV2Command::ResolvePrompt {
            user_reply, sub_key, job_id, role, agent_id, source_event,
        } => handle_resolve_prompt(user_reply, sub_key, job_id, role, agent_id, source_event),
        PendingDecisionsV2Command::Pick { index } => handle_pick(index),
        PendingDecisionsV2Command::List { format } => handle_list(format),
        PendingDecisionsV2Command::Cancel { sub_key, index } => handle_cancel(sub_key, index),
    }
}

// ─── Handlers ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
#[allow(unreachable_code)]
fn handle_request(
    sub_key: String,
    job_id: String,
    role: String,
    agent_id: String,
    user_content: String,
    list_label: String,
    llm_content: Option<String>,
    source_event: Option<String>,
) -> Result<()> {
    // CLI mode: the driver (a non-MCP CLI loop) owns turn-taking and doesn't need
    // queue routing. Bypass the queue file entirely — build an ad-hoc entry and
    // emit playbook_push so the LLM calls xmtp_prompt_user immediately.
    let cli_mode_env = std::env::var("OKX_A2A_IS_CLI").unwrap_or_default();
    let cli_mode = cli_mode_env == "1";
    trace_log(&format!(
        "handle_request {} (OKX_A2A_IS_CLI={:?}): job_id={} role={} sub_key={}",
        if cli_mode { "CLI_MODE" } else { "QUEUE_MODE" },
        cli_mode_env, job_id, role, sub_key,
    ));
    if cli_mode {
        let now = Utc::now();
        let entry = PendingEntry {
            sub_key,
            job_id,
            role,
            agent_id,
            user_content,
            list_label,
            llm_content_override: llm_content,
            source_event,
            status: Status::Active,
            created_at: now,
            updated_at: now,
        };
        print!("{}", playbook_push_cli(&entry));
        return Ok(());
    }

    // Non-CLI mode: emit the `prompt user` CLI subprocess playbook (same shape
    // as playbook_push_cli, just a different binary). Like the cli_mode branch,
    // this also bypasses the queue file entirely — build an ad-hoc entry and
    // return the playbook directly. The queue-file logic further below is kept
    // for reference but currently unreachable.
    {
        let now = Utc::now();
        let entry = PendingEntry {
            sub_key,
            job_id,
            role,
            agent_id,
            user_content,
            list_label,
            llm_content_override: llm_content,
            source_event,
            status: Status::Active,
            created_at: now,
            updated_at: now,
        };
        print!("{}", playbook_push_prompt_user(&entry));
        return Ok(());
    }

    // Reject hallucinated sub_key shapes early. The only valid sub_key is the
    // full XMTP sessionKey returned by `session_status` — anything else (e.g.
    // `review-<jobId>`, the bare jobId, a label) silently breaks `xmtp_dispatch_session`
    // routing later because xmtp's session registry cannot resolve it. Real
    // incident: a Minimax model skipped `session_status` and made up
    // `--sub-key "review-<jobId>"`; the relay never reached the actual sub.
    if let Err(msg) = validate_sub_key(&sub_key, &job_id) {
        anyhow::bail!(
            "Invalid --sub-key: {}\n\n\
             Two valid shapes (both must contain `:okx-a2a:group:`):\n\
               • task sub (after xmtp_start_conversation with a peer):\n  \
                 agent:main:okx-a2a:group:okx-xmtp:my=0x...&to=0x...&job=<jobId>&gid=...\n\
               • backup sub (per-jobId) (handles chain events for this agent BEFORE a task sub exists,\n\
                 e.g. job_created):\n  \
                 agent:main:okx-a2a:group:okx-xmtp:backup:<jobId>\n\n\
             Fix: call `session_status` (xmtp tool) FIRST to obtain the current sessionKey, \
             then pass the verbatim returned string as --sub-key. Do NOT invent prefixes \
             (`review-`, `decision-`, the jobId alone, list labels, …) — they will silently \
             break dispatch routing.",
            msg
        );
    }

    let _lock = acquire_lock()?;
    let mut q = read_queue()?;
    ensure_invariant_and_evict(&mut q);

    let prev_idx = q.entries.iter().position(|e| e.sub_key == sub_key);
    let (new_status, original_created_at) = match prev_idx {
        Some(idx) => {
            let old = &q.entries[idx];
            (old.status.clone(), old.created_at)
        }
        None => {
            let status = if q.entries.iter().any(|e| e.status == Status::Active) {
                Status::Queued
            } else {
                Status::Active
            };
            (status, Utc::now())
        }
    };

    // Re-prompt the active card whenever this request lands as queued — regardless of
    // whether the sub_key is new or an overwrite. Same sub re-asking still surfaces
    // the active card (defensive against the active getting buried by intermediate chat).
    let active_for_reprompt: Option<PendingEntry> = if new_status == Status::Queued {
        q.entries
            .iter()
            .find(|e| e.status == Status::Active)
            .cloned()
    } else {
        None
    };

    if let Some(idx) = prev_idx {
        q.entries.remove(idx);
    }
    q.entries.push(PendingEntry {
        sub_key: sub_key.clone(),
        job_id: job_id.clone(),
        role: role.clone(),
        agent_id,
        user_content: user_content.clone(),
        list_label,
        llm_content_override: llm_content,
        source_event,
        status: new_status.clone(),
        created_at: original_created_at,
        updated_at: Utc::now(),
    });

    write_queue_atomic(&q)?;

    match new_status {
        Status::Active => {
            let entry = q.entries.last().unwrap();
            print!("{}", playbook_push(entry));
        }
        Status::Queued => {
            let pos = q
                .entries
                .iter()
                .filter(|e| e.status == Status::Queued)
                .count();
            if let Some(active) = active_for_reprompt {
                let new_entry = q.entries.last().unwrap();
                print!(
                    "{}",
                    playbook_wait_with_reprompt(&active, new_entry, pos)
                );
            } else {
                print!("{}", playbook_wait(pos));
            }
        }
    }
    Ok(())
}

/// CLI-driver bypass: build the full system-shaped relay envelope from the
/// caller-supplied routing fields and emit `playbook_relay_only`. Mirrors the
/// queue-based `handle_resolve` envelope shape exactly (same fields, same
/// `user_decision_<source_event>` event), so the receiving sub routes via the
/// same `next-action --event user_decision_<X>` handler regardless of mode.
fn handle_resolve_with_sessionkey(
    user_reply: String,
    sub_key: String,
    job_id: String,
    role: String,
    agent_id: String,
    source_event: String,
) -> Result<()> {
    trace_log(&format!(
        "handle_resolve_with_sessionkey: sub_key={} job_id={} role={} agent_id={} source_event={} user_reply={:?}",
        sub_key, job_id, role, agent_id, source_event, user_reply,
    ));
    let relay_event = format!("user_decision_{}", source_event);
    let description = format!(
        "User-decision relay envelope (CLI mode). Call `onchainos agent next-action \
         --jobid {jid} --event {evt} --role {role} --agentId {agent} \
         --data \"<message.data verbatim>\"` to fetch the routing playbook; follow it. \
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
    print!("{}", playbook_relay_only_cli(&sub_key, &relay_content));
    Ok(())
}

/// MCP variant of `handle_resolve_with_sessionkey`. Builds the same
/// system-shaped relay envelope from the caller-supplied routing fields, but
/// emits `playbook_relay_only_prompt` (MCP `xmtp_dispatch_session` tool call)
/// instead of `playbook_relay_only_cli` (`okx-a2a session send` bash). Pairs
/// with `playbook_push_prompt_user` so an MCP push lands an MCP relay.
fn handle_resolve_prompt(
    user_reply: String,
    sub_key: String,
    job_id: String,
    role: String,
    agent_id: String,
    source_event: String,
) -> Result<()> {
    trace_log(&format!(
        "handle_resolve_prompt: sub_key={} job_id={} role={} agent_id={} source_event={} user_reply={:?}",
        sub_key, job_id, role, agent_id, source_event, user_reply,
    ));
    let relay_event = format!("user_decision_{}", source_event);
    let description = format!(
        "User-decision relay envelope (MCP prompt mode). Call `onchainos agent next-action \
         --jobid {jid} --event {evt} --role {role} --agentId {agent} \
         --data \"<message.data verbatim>\"` to fetch the routing playbook; follow it. \
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
    print!("{}", playbook_relay_only_prompt(&sub_key, &relay_content));
    Ok(())
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
         --jobid {jid} --event {evt} --role {role} --agentId {agent} \
         --data \"<message.data verbatim>\"` to fetch the routing playbook; follow it. \
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
        print!("{}", playbook_relay_only(&active.sub_key, &relay_content));
        write_queue_atomic(&q)?;
    } else {
        // Auto-advance: promote the newest queued entry (LIFO — sort already placed it at
        // index 0 since the active was just removed). Render the new active + the remaining
        // list in one go so the user sees the next decision immediately, no extra round-trip
        // through "selection mode".
        //
        // Promote by sub_key (not by raw position) to be robust against any reordering.
        let promote_sub_key = queued[0].sub_key.clone();
        let promote_idx = q
            .entries
            .iter()
            .position(|e| e.sub_key == promote_sub_key)
            .unwrap();
        q.entries[promote_idx].status = Status::Active;
        // Re-sort so the newly-promoted active sits at index 0 (the sort honors the
        // "active first, then LIFO" invariant).
        ensure_invariant_and_evict(&mut q);

        let snap = build_snapshot(&q);
        write_snapshot_atomic(&snap)?;
        write_queue_atomic(&q)?;

        print!(
            "{}",
            playbook_relay_and_advance(&active.sub_key, &relay_content, &q)
        );
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

    let target_sub_key = snapshot.items[index - 1].sub_key.clone();
    let snap_displayed_at = snapshot.displayed_at;

    let entry_idx = q.entries.iter().position(|e| e.sub_key == target_sub_key);
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

    // Three cases by current status:
    //   (a) The picked entry IS already active → re-render its card (no state change).
    //       User likely wants to re-see the card after scrolling past it.
    //   (b) The picked entry is queued AND no active exists → promote it (selection-mode flow).
    //   (c) The picked entry is queued AND a DIFFERENT entry is currently active →
    //       **swap**: demote the current active to queued, promote the picked one to active.
    //       Neither decision is lost; the user can come back to either by `pick --index <N>`.
    let already_active = q.entries[entry_idx].status == Status::Active;
    let picked_sub_key = q.entries[entry_idx].sub_key.clone();
    if !already_active {
        // If another entry is currently active, demote it to queued (swap, not drop).
        for e in q.entries.iter_mut() {
            if e.status == Status::Active {
                e.status = Status::Queued;
            }
        }
        q.entries[entry_idx].status = Status::Active;
        // Re-sort so the newly-promoted active sits at index 0 (active-first invariant).
        ensure_invariant_and_evict(&mut q);
        // Refresh snapshot so a subsequent `pick --index N` reflects the new order
        // (otherwise the next pick would resolve indices against the stale pre-pick layout).
        let new_snap = build_snapshot(&q);
        write_snapshot_atomic(&new_snap)?;
        write_queue_atomic(&q)?;
    }
    // entry_idx may now be invalid after the sort — look up the entry by its sub_key.
    let render_idx = q
        .entries
        .iter()
        .position(|e| e.sub_key == picked_sub_key)
        .expect("picked entry must still exist after promotion + sort");
    print!("{}", playbook_render(&q.entries[render_idx]));
    Ok(())
}

fn handle_cancel(
    sub_key: Option<String>,
    index: Option<usize>,
) -> Result<()> {
    let _lock = acquire_lock()?;
    let mut q = read_queue()?;
    ensure_invariant_and_evict(&mut q);

    // Resolve target sub_key (one of --sub-key / --index)
    let target_sub_key = match (sub_key, index) {
        (Some(sk), None) => sk,
        (None, Some(idx)) => {
            let snapshot = read_snapshot();
            if idx == 0 || idx > snapshot.items.len() {
                let new_snap = build_snapshot(&q);
                write_snapshot_atomic(&new_snap)?;
                print!(
                    "{}",
                    playbook_stale_relist(&new_snap, "cancel index out of range")
                );
                return Ok(());
            }
            snapshot.items[idx - 1].sub_key.clone()
        }
        (Some(_), Some(_)) => bail!("--sub-key and --index are mutually exclusive"),
        (None, None) => bail!("must provide either --sub-key or --index"),
    };

    // Locate + remove
    let Some(entry_idx) = q.entries.iter().position(|e| e.sub_key == target_sub_key) else {
        print!(
            "{}",
            playbook_error(&format!(
                "no pending decision found for sub_key: {}",
                target_sub_key
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
        let newest_queued_sub_key = q
            .entries
            .iter()
            .filter(|e| e.status == Status::Queued)
            .max_by_key(|e| e.created_at)
            .map(|e| e.sub_key.clone());
        if let Some(sk) = newest_queued_sub_key {
            if let Some(promote_idx) = q.entries.iter().position(|e| e.sub_key == sk) {
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
                    "sub_key": e.sub_key,
                    "job_id": e.job_id,
                    "role": e.role,
                    "agent_id": e.agent_id,
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
/// Callers (`handle_list`, `playbook_relay_and_advance`, `playbook_cancel`) wrap
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
                sub_key: e.sub_key.clone(),
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
/// (`xmtp_prompt_user` / `xmtp_dispatch_user` / end-turn) and explicitly warns that
/// stdout IS the next-action playbook (not log output). Without this, smaller models
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
    user_content: &str,
    list_label_full: &str,
    source_event: &str,
) -> String {
    // Bash `--user-content "..."` uses double quotes; escape `\` and `"` inside.
    let user_content_escaped = user_content
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!(
        "Execute these 5 sub-steps strictly in order. ALL FIVE are mandatory; skipping any breaks the flow.\n\n\
         **(1) Get sessionKey** — call `session_status` once this turn; pass the returned key as `--sub-key` in step (3). Do NOT invent prefixes (`review-`, `decision-`, the raw jobId, etc.) — those silently break dispatch routing.\n\n\
         **(2) Translate `--user-content` AND `--list-label` to the user's language BEFORE step (3)** (signal = user's OWN typed messages this session; default English if unsure; see [Localization] above for token mapping). The bash in (3) shows English placeholders for shape reference only — the actual strings you pass MUST be localized. Keep `--list-label`'s bracket prefix structure (e.g. `[Decision <shortJobId>]`) intact: translate the keyword inside the bracket and the suffix phrase, but preserve the shortJobId hex.\n\n\
         **(3) Run `pending-decisions-v2 request`** using sessionKey from (1) and translated args from (2):\n\
         ```bash\n\
         onchainos agent pending-decisions-v2 request \\\n\
         \x20\x20--sub-key \"<full sessionKey from step 1>\" \\\n\
         \x20\x20--job-id {job_id} --role {role} --agent-id {agent_id} \\\n\
         \x20\x20--user-content \"{content}\" \\\n\
         \x20\x20--list-label \"{label}\" \\\n\
         \x20\x20--source-event {source_event}\n\
         ```\n\n\
         **(4) Read step (3)'s stdout and follow it verbatim.** 🛑 The printed text IS your next-action playbook (it self-describes: tells you which xmtp tool to call with which args, or to end the turn) — it is NOT a success-confirmation receipt. Skipping (4) = card never reaches the user → flow stalls → 24h auto-refund / mistaken auto-decline. Do NOT hand-craft `llmContent` or call `xmtp_dispatch_session` yourself — that path is owned by `pending-decisions-v2` now.\n\n\
         **(5) End the turn** after (4)'s tool call returns 'sent' (or immediately if (4) was the no-tool branch). Do NOT call further tools, do NOT loop back to (3).\n",
        job_id = job_id,
        role = role,
        agent_id = agent_id,
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

/// Validate that `sub_key` is a real XMTP sessionKey rather than a hallucinated
/// stand-in like `review-<jobId>` / the bare jobId / a list label. Required
/// Two valid shapes (both belong to the OKX a2a group namespace):
/// - **task sub** (after `xmtp_start_conversation` with a peer): `agent:...:okx-a2a:group:okx-xmtp:my=...&to=...&job=<job_id>&gid=...`
/// - **backup sub (per-jobId)** (handles events before a task sub exists, e.g. `job_created`):
///   `agent:...:okx-a2a:group:okx-xmtp:backup:<jobId>` (no `&job=`)
///
/// Both share the `agent:` prefix and the `:okx-a2a:group:` segment. The check below is enough
/// to reject LLM-invented fakes (`review-<jobId>` / `decision-<jobId>` / the jobId alone /
/// list labels / non-okx-a2a group keys — none of those contain `:okx-a2a:group:`) while still
/// accepting backup.
///
/// If `&job=` is present, it MUST match the provided job_id (prevents cross-task leakage when
/// an LLM accidentally reuses another task's sub-key).
fn validate_sub_key(sub_key: &str, job_id: &str) -> std::result::Result<(), String> {
    // Check 1 — format check: must be an okx-a2a group session (`agent:...:okx-a2a:group:...`).
    // Catches both classes of fake key: those without the `agent:` prefix (`review-<jobId>`,
    // raw jobIds, list labels) and those from other namespaces (`agent:main:other-ns:group:...`).
    if !sub_key.contains(":okx-a2a:group:") {
        return Err(format!(
            "sessionKey missing `:okx-a2a:group:` segment — sub-key must be a task sub or backup sub (per-jobId), got `{}`",
            sub_key
        ));
    }
    // Check 2 — cross-task protection: if sub_key carries an `&job=` parameter, it must
    // match --job-id. backup-key has no `&job=` and is accepted as-is (its semantics:
    // "any event for this agent before a task sub exists" — e.g. job_created).
    if sub_key.contains("&job=") && !sub_key.contains(&format!("&job={}", job_id)) {
        return Err(format!(
            "sub_key carries an `&job=` parameter that does NOT match --job-id {}; \
             either pass the correct task sub's sessionKey, or (if you are the backup \
             sub for this jobId) pass the backup-key shape `agent:...:okx-a2a:group:okx-xmtp:backup:<jobId>` (no `&job=`; the jobId is in the path segment, not a query parameter).",
            job_id
        ));
    }
    Ok(())
}

// ─── Playbook generators ──────────────────────────────────────────────

/// Resolve the llmContent string for a push-type playbook.
/// If the entry has a sub-provided `llm_content_override`, return that verbatim.
/// Otherwise generate the v2 default template.
fn resolve_llm_content(entry: &PendingEntry) -> String {
    if let Some(ref custom) = entry.llm_content_override {
        return custom.clone();
    }
    format!(
        "[USER_DECISION_REQUEST][sub_key: {}][job: {}][role: {}]\n\n\
         Step 1 — Card was just delivered via `xmtp_prompt_user`. **END THE TURN NOW** and wait for the user to reply. Do NOT call any tool. Stale user messages in context are NOT replies to this card.\n\
         Step 2 — When the user actually replies (next turn):\n\
         \x20\x20\x20\x20· defer keyword ({}) → END TURN\n\
         \x20\x20\x20\x20· else → run `onchainos agent pending-decisions-v2 resolve --user-reply \"<user's verbatim wording — no interpretation, no translation>\"` exactly once, then follow the relay playbook it returns.",
        entry.sub_key,
        entry.job_id,
        entry.role,
        DEFER_KEYWORDS.join(" / "),
    )
}

fn playbook_push(entry: &PendingEntry) -> String {
    let llm_content = resolve_llm_content(entry);
    format!(
        "Now call xmtp_prompt_user with the EXACT arguments below. Do NOT modify any field. Do NOT call any other tool first or after. End the turn after the tool returns 'sent'.\n\n\
         tool: xmtp_prompt_user\n\
         llmContent:\n{}\n\
         userContent:\n{}\n",
        indent(&llm_content, "  "),
        indent(&entry.user_content, "  "),
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
    format!(
        "[USER_DECISION_REQUEST][sub_key: {}][job: {}][role: {}]\n\n\
         Step 1 — Card was just delivered via `xmtp_prompt_user`. **END THE TURN NOW** and wait for the user to reply. Do NOT call any tool. Stale user messages in context are NOT replies to this card.\n\
         Step 2 — When the user actually replies (next turn):\n\
         \x20\x20\x20\x20· defer keyword ({}) → END TURN\n\
         \x20\x20\x20\x20· else → run `onchainos agent pending-decisions-v2 resolve-with-sessionkey --user-reply \"<user's verbatim wording — no interpretation, no translation>\" --sub-key \"{}\" --job-id \"{}\" --role \"{}\" --agent-id \"{}\" --source-event \"{}\"` exactly once, then follow the relay playbook it returns.",
        entry.sub_key,
        entry.job_id,
        entry.role,
        DEFER_KEYWORDS.join(" / "),
        entry.sub_key,
        entry.job_id,
        entry.role,
        entry.agent_id,
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
    format!(
        "[USER_DECISION_REQUEST]\n\
         [sub_key: {sub}][job: {job}][role: {role}]\n\
         (Anything above this marker is stale — NOT a reply to this card.)\n\n\
         Step 1 — Card just delivered.\n\n\
         Step 2 — Scan your current context for OTHER [USER_DECISION_REQUEST] blocks. \
         If you find any, render the warning below to the user as your assistant response (in user's language), e.g.:\n\
         \x20\x20`⚠️ You have multiple decisions pending — please prefix your reply with the jobId short hash, e.g. \\`0x7091: approve\\`, so it routes correctly.`\n\
         If no other blocks → skip this step.\n\n\
         Step 3 — **END THE TURN NOW**, wait for user reply. Do NOT call any tool.\n\n\
         🛑 **The block below runs ONLY in a future turn**, AFTER the user has actually replied. Do NOT run anything in the current turn.\n\
         On the user's next reply, re-scan your context for [USER_DECISION_REQUEST] blocks (the count may have changed since Step 2), then walk this decision tree:\n\
         \x20\x20· defer keyword ({defer}) → END TURN, do NOT run anything.\n\
         \x20\x20· Reply starts with `0x...:` prefix → strip the prefix + colon, use the prefix to match each block's `[job: 0x...]` header, locate THAT block, then run THAT block's command template with `--user-reply` set to the stripped wording (without the prefix).\n\
         \x20\x20· No prefix + only THIS block in context (single) → run THIS block's command template with the full reply.\n\
         \x20\x20· 🔁 No prefix + **multiple** [USER_DECISION_REQUEST] blocks in context → user forgot to add the jobId prefix. Ask them which jobId they're answering (number the candidates `1. Job 0x...`, `2. Job 0x...`, one per line — short_jobId only), **END THE TURN**, wait for the pick (hex prefix `0x7091` or list number `1`); locate THAT block via `[job: 0x...]` header (or list order), then run THAT block's command template. Never guess, never collapse.\n\n\
         **Command template** (pre-filled for THIS block; only run AFTER the user has replied):\n\
         \x20\x20`onchainos agent pending-decisions-v2 resolve-prompt --user-reply \"<user wording, without any jobId prefix>\" --sub-key \"{sub}\" --job-id \"{job}\" --role \"{role}\" --agent-id \"{agent}\" --source-event \"{src}\"`\n\n\
         After running, follow the relay playbook the command returns.",
        sub = entry.sub_key,
        job = entry.job_id,
        role = entry.role,
        agent = entry.agent_id,
        src = source_event_str,
        defer = DEFER_KEYWORDS.join(" / "),
    )
}

/// CLI-driver variant of `playbook_push`. Uses the `okx-a2a user decision-request`
/// CLI subprocess (writes SQLite `user_attention`) instead of the MCP-only
/// `xmtp_prompt_user` tool, since CLI mode runs outside of an MCP host.
fn playbook_push_cli(entry: &PendingEntry) -> String {
    let llm_content = resolve_llm_content_cli(entry);
    // Single-quote the bash args; only `'` itself needs escaping via the canonical `'\''` trick.
    let user_content_q = entry.user_content.replace('\'', "'\\''");
    let llm_content_q = llm_content.replace('\'', "'\\''");
    format!(
        "Now run the EXACT CLI command below. Do NOT modify any field. Do NOT call any other tool first or after. End the turn after the command returns.\n\n\
         ```bash\n\
         okx-a2a user decision-request \\\n\
         \x20\x20--user-content '{user}' \\\n\
         \x20\x20--llm-content '{llm}' \\\n\
         \x20\x20--json\n\
         ```\n",
        user = user_content_q,
        llm = llm_content_q,
    )
}

/// Default (non-CLI) variant of `playbook_push`. Same MCP `xmtp_prompt_user`
/// tool invocation shape as `playbook_push`; the only difference is the
/// embedded llmContent comes from `resolve_llm_content_prompt_user` (with
/// multi-decision disambiguation). Used by `handle_request` when
/// `OKX_A2A_IS_CLI` is NOT set.
fn playbook_push_prompt_user(entry: &PendingEntry) -> String {
    let llm_content = resolve_llm_content_prompt_user(entry);
    format!(
        "Now call xmtp_prompt_user with the EXACT arguments below. Do NOT modify any field. Do NOT call any other tool first or after. End the turn after the tool returns 'sent'.\n\n\
         tool: xmtp_prompt_user\n\
         llmContent:\n{}\n\
         userContent:\n{}\n",
        indent(&llm_content, "  "),
        indent(&entry.user_content, "  "),
    )
}

fn playbook_wait(position: usize) -> String {
    format!(
        "The user is currently answering a prior decision. Your decision is queued (position {}).\n\n\
         Do NOT call any xmtp tool. End the turn now. The CLI will auto-render your prompt when it becomes active.\n",
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
    // body to match the user's language before xmtp_dispatch_user. We do NOT
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
         **Step 2 — Call `xmtp_dispatch_user` with the localized content from Step 1**:\n\
         \x20\x20tool: xmtp_dispatch_user\n\
         \x20\x20arguments:\n\
         \x20\x20\x20\x20content: <the localized Step 1 output>\n\n\
         End the turn after the tool returns. Do NOT call any other tool first or after.\n",
        pos = queued_position,
        body = dispatch_content,
    )
}

fn playbook_relay_only(sub_key: &str, relay_content: &str) -> String {
    format!(
        "Relay the user's decision to the just-resolved sub session, then end the turn.\n\n\
         tool: xmtp_dispatch_session\n\
         sessionKey: {}\n\
         content: {}\n\n\
         ⚠️ Call `xmtp_dispatch_session` **exactly once**, then end the turn. Repeat = recursion loop; skip = task stalls.\n\
         🛑 User reply consumed — do NOT reuse it for future cards; wait for a fresh user message.\n",
        sub_key, relay_content
    )
}

/// MCP variant of `playbook_relay_only_cli` — same `xmtp_dispatch_session` tool
/// invocation as `playbook_relay_only`, just with the CONSUMPTION MARKER's
/// command name pinned to `resolve-prompt` so the LLM doesn't accidentally
/// retry against another resolver. Pairs with `handle_resolve_prompt` /
/// `playbook_push_prompt_user` for the MCP push → MCP relay round-trip.
fn playbook_relay_only_prompt(sub_key: &str, relay_content: &str) -> String {
    format!(
        "Relay the user's decision to the just-resolved sub session, then end the turn.\n\n\
         tool: xmtp_dispatch_session\n\
         sessionKey: {}\n\
         content: {}\n\n\
         ⚠️ Call `xmtp_dispatch_session` **exactly once**, then end the turn. Repeat = recursion loop; skip = task stalls.\n\
         🛑 User reply consumed — do NOT reuse it (no `resolve-prompt` retry, no future-card reference); wait for a fresh user message.\n",
        sub_key, relay_content
    )
}

/// CLI-driver variant of `playbook_relay_only`. Uses the `okx-a2a session send`
/// CLI subprocess instead of the MCP-only `xmtp_dispatch_session` tool, since
/// CLI mode runs outside of an MCP host. Same semantics: relay once, then end.
fn playbook_relay_only_cli(sub_key: &str, relay_content: &str) -> String {
    // Single-quote the bash args; only `'` itself needs escaping via the canonical `'\''` trick.
    let sub_key_q = sub_key.replace('\'', "'\\''");
    let relay_content_q = relay_content.replace('\'', "'\\''");
    format!(
        "Relay the user's decision to the just-resolved sub session.\n\n\
         ```bash\n\
         okx-a2a session send \\\n\
         \x20\x20--session-key '{key}' \\\n\
         \x20\x20--content '{content}' \\\n\
         \x20\x20--no-wait --json\n\
         ```\n\n\
         ⚠️ Run this command **exactly once**, then end the turn. Repeat = recursion loop; skip = task stalls.\n\
         🛑 User reply consumed — do NOT reuse it (no `resolve-with-sessionkey` retry, no future-card reference); wait for a fresh user message.\n",
        key = sub_key_q,
        content = relay_content_q,
    )
}

/// Resolve auto-advance playbook: relay user's reply to the just-resolved sub, then render
/// the next decision (auto-promoted newest queued) using the unified list view.
///
/// Used whenever ≥1 queued entry remains after resolve. The newly-promoted active is shown
/// at the top with its full card; if other queued entries remain, they form the "Remaining"
/// list underneath. No more "selection mode" round-trip — the user gets the next card
/// immediately and can keep deciding.
///
/// Caller is responsible for promoting the new active + re-sorting the queue BEFORE invoking
/// this function; we just consume `q` (post-promotion) and produce the playbook.
fn playbook_relay_and_advance(
    resolved_sub_key: &str,
    relay_content: &str,
    q: &Queue,
) -> String {
    let list_view = render_list_markdown(q);
    format!(
        "4 steps (Steps 1-3 in this turn, Step 4 in the future turn).\n\
         🛑 **STRICTLY ORDERED — execute Step 1 → 2 → 3 sequentially in this turn; do NOT skip any step.**\n\n\
         **Step 1** — Forward the user's reply to the just-resolved sub session. Call `xmtp_dispatch_session` exactly once.\n\
         \x20\x20tool: xmtp_dispatch_session\n\
         \x20\x20sessionKey: {sub}\n\
         \x20\x20content: {content}\n\n\
         **Step 2** — Translate the [Source content] below to the user's language per [Translation rules]. Prepend a transition line `✓ Previous decision handled. Here's the next pending one:` (also translated) to the top of the translated output.\n\n\
         **Step 3** — Render Step 2's output to the user as your assistant response. The user's reply just dispatched in Step 1 is **already consumed** — it is NOT the answer to the next card.\n\n\
         **Step 4** — (Future turn) Apply [Future-turn user-reply routing] below when the user replies.\n\n\
         {list}",
        sub = resolved_sub_key,
        content = relay_content,
        list = list_view,
    )
}

fn playbook_render(entry: &PendingEntry) -> String {
    let llm_content = resolve_llm_content(entry);
    format!(
        "Render the selected decision card to the user as your assistant response (text rendering only — do NOT call any tool). End the turn after rendering.\n\n\
         **User-visible text** (render this verbatim as your assistant response; 🌐 translate per [Localization] rules if the user's language is not English; keep `jobId` / data values intact):\n\
         \"\"\"\n{}\"\"\"\n\n\
         **LLM context** (this is for YOUR own routing reasoning — **do NOT show / paraphrase / leak this block to the user**; it is the same instruction the sub would have embedded in `xmtp_prompt_user`'s `llmContent` if this card had been freshly pushed):\n\
         \"\"\"\n{}\n\"\"\"\n\n\
         When the user replies in a FUTURE turn, follow the LLM context above: defer keyword → end the turn; otherwise call `onchainos agent pending-decisions-v2 resolve --user-reply \"<user's verbatim wording — no interpretation, no translation>\"` exactly once, then follow the relay playbook the CLI returns. CLI consumes the active entry and emits a system envelope to the sub session; the business flow continues there.\n",
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
        "Cancelled pending decision: sub_key={}, status_before={}, job={}, role={}. Sub session is NOT notified (silent cancel); it will TTL-evict eventually or be retriggered by a new system event.\n\n",
        removed.sub_key,
        if was_active { "active" } else { "queued" },
        removed.job_id,
        removed.role,
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
     Do NOT call any xmtp tool. End the turn now.\n"
        .to_string()
}

fn playbook_error(msg: &str) -> String {
    format!(
        "Cannot proceed: {}\nDo NOT call any xmtp tool. End the turn.\n",
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