//! Pending-decisions v2 — redesigned queue with single-active invariant,
//! implicit state machine, sessionKey primary key, and LLM-playbook output.
//!
//! See the design doc at:
//!   https://okg-block.sg.larksuite.com/docx/URN9d8q49oYAJnxH6BYlYTkUgkd
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

    // 3a. Normalize entry order to FIFO by created_at. Overwrites via Vec::push move entries
    //     to the tail; without this sort, the queue file and snapshot would show jumpy order.
    queue.entries.sort_by_key(|e| e.created_at);

    // 3b. If eviction killed the active entry, promote the oldest queued to recover.
    //    NOTE: only triggers when `evicted > 0`. Otherwise "no active + N queued" is a
    //    valid state (selection mode after resolve with queue >= 2) and must be preserved.
    if evicted > 0 {
        let has_active = queue.entries.iter().any(|e| e.status == Status::Active);
        if !has_active {
            if let Some(oldest) = queue
                .entries
                .iter_mut()
                .filter(|e| e.status == Status::Queued)
                .min_by_key(|e| e.created_at)
            {
                oldest.status = Status::Active;
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
    },

    /// (user-session) Resolve the current active decision with user's reply.
    Resolve {
        #[arg(long = "user-reply")]
        user_reply: String,
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
        } => handle_request(sub_key, job_id, role, agent_id, user_content, list_label, llm_content),
        PendingDecisionsV2Command::Resolve { user_reply } => handle_resolve(user_reply),
        PendingDecisionsV2Command::Pick { index } => handle_pick(index),
        PendingDecisionsV2Command::List { format } => handle_list(format),
    }
}

// ─── Handlers ──────────────────────────────────────────────────────────

fn handle_request(
    sub_key: String,
    job_id: String,
    role: String,
    agent_id: String,
    user_content: String,
    list_label: String,
    llm_content: Option<String>,
) -> Result<()> {
    // Reject hallucinated sub_key shapes early. The only valid sub_key is the
    // full XMTP sessionKey returned by `session_status` — anything else (e.g.
    // `review-<jobId>`, the bare jobId, a label) silently breaks `xmtp_dispatch_session`
    // routing later because xmtp's session registry cannot resolve it. Real
    // incident: a Minimax model skipped `session_status` and made up
    // `--sub-key "review-<jobId>"`; the relay never reached the actual sub.
    if let Err(msg) = validate_sub_key(&sub_key, &job_id) {
        anyhow::bail!(
            "Invalid --sub-key: {}\n\n\
             The only valid value is the full XMTP sessionKey for the current sub session, e.g.:\n  \
             agent:main:okx-a2a:group:okx-xmtp:my=0x...&to=0x...&job=<jobId>&gid=...\n\n\
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
    // Two relay shapes coexist:
    //  - v1 intent-tag scenes (e.g. JobRefused with `--llm-content` instructing the user-session
    //    to call `resolve --user-reply "[intent:CODE] user said: ..."`): concat directly so the
    //    final content is `[USER_DECISION_RELAY][intent:CODE] user said: ...` — sub-side flow.rs
    //    Step 2 branches on `[intent:CODE]`.
    //  - default v2 scenes (raw verbatim user wording): prepend `decision: ` so the content is
    //    `[USER_DECISION_RELAY] decision: <verbatim>` — matches the documented skill prefix.
    let relay_content = if user_reply.starts_with("[intent:") {
        format!("[USER_DECISION_RELAY]{}", user_reply)
    } else {
        format!("[USER_DECISION_RELAY] decision: {}", user_reply)
    };

    let queued: Vec<&PendingEntry> = q
        .entries
        .iter()
        .filter(|e| e.status == Status::Queued)
        .collect();

    match queued.len() {
        0 => {
            print!("{}", playbook_relay_only(&active.sub_key, &relay_content));
            write_queue_atomic(&q)?;
        }
        1 => {
            let promote_sub_key = queued[0].sub_key.clone();
            let promote_idx = q
                .entries
                .iter()
                .position(|e| e.sub_key == promote_sub_key)
                .unwrap();
            q.entries[promote_idx].status = Status::Active;
            print!(
                "{}",
                playbook_relay_and_render(
                    &active.sub_key,
                    &relay_content,
                    &q.entries[promote_idx]
                )
            );
            write_queue_atomic(&q)?;
        }
        _n => {
            // Refresh snapshot for the list display
            let snap = build_snapshot(&q);
            write_snapshot_atomic(&snap)?;
            write_queue_atomic(&q)?;
            print!(
                "{}",
                playbook_relay_and_list(&active.sub_key, &relay_content, &snap)
            );
        }
    }
    Ok(())
}

fn handle_pick(index: usize) -> Result<()> {
    let _lock = acquire_lock()?;
    let mut q = read_queue()?;
    ensure_invariant_and_evict(&mut q);

    if q.entries.iter().any(|e| e.status == Status::Active) {
        print!("{}", playbook_error("There is already an active decision; resolve it first before picking from list."));
        return Ok(());
    }

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

    q.entries[entry_idx].status = Status::Active;
    write_queue_atomic(&q)?;
    print!("{}", playbook_render(&q.entries[entry_idx]));
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
            if q.entries.is_empty() {
                println!("(no pending decisions)");
            } else {
                println!("{} pending decision(s):\n", q.entries.len());
                println!("| # | Active | Role     | Job             | Label                            |");
                println!("|---|--------|----------|-----------------|----------------------------------|");
                for (i, e) in q.entries.iter().enumerate() {
                    let active = if e.status == Status::Active { "✓" } else { " " };
                    let short_job = short_job_id(&e.job_id);
                    println!(
                        "| {} | {}      | {:<8} | {} | {} |",
                        i + 1,
                        active,
                        e.role,
                        short_job,
                        e.list_label
                    );
                }
            }
        }
    }
    Ok(())
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

/// Validate that `sub_key` is a real XMTP sessionKey rather than a hallucinated
/// stand-in like `review-<jobId>` / the bare jobId / a list label. Required
/// shape: starts with `agent:`, contains `&job=<job_id>`, contains `&gid=`.
fn validate_sub_key(sub_key: &str, job_id: &str) -> std::result::Result<(), String> {
    if !sub_key.starts_with("agent:") {
        return Err(format!(
            "expected a sessionKey starting with `agent:`, got `{}`",
            sub_key
        ));
    }
    let expected_job = format!("&job={}", job_id);
    if !sub_key.contains(&expected_job) {
        return Err(format!(
            "sessionKey does not contain `{}` — sub_key's job must match --job-id",
            expected_job
        ));
    }
    if !sub_key.contains("&gid=") {
        return Err("sessionKey missing `&gid=` segment".to_string());
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
         Check the most recent message in context:\n\
         \x20\x20• tool_result (card just sent) → END TURN. Do NOT call resolve.\n\
         \x20\x20• fresh user_message (user actually replied):\n\
         \x20\x20\x20\x20· defer keyword ({}) → END TURN\n\
         \x20\x20\x20\x20· else → `resolve --user-reply \"<verbatim, no interpretation>\"`",
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
    // llmContent routes future user reply to the ACTIVE entry — user-session's
    // `resolve` always targets the active, regardless of which sub displayed
    // the prompt. This preserves the single-active invariant.
    let llm_content = resolve_llm_content(active);
    // Canonical English wrapper. Sub LLM translates the wrapper lines (the 🆕
    // top notice + the `─── Current active decision ───` divider) to match the
    // embedded active card's language. The embedded `active.user_content` is
    // already in the user's language (sub-A localized at request time when the
    // active was first pushed); do NOT re-translate it. The `new_entry.list_label`
    // is sub-provided and already localized too; do NOT translate it.
    let user_content_wrapped = format!(
        "🆕 **A new decision arrived (queued: \"{}\") — {} pending decisions in total. \
         Please answer the active decision below first; the queued one will auto-display \
         once you finish this.**\n\n\
         ─────────── Current active decision ───────────\n\
         {}",
        new_entry.list_label, total_pending, active.user_content,
    );
    format!(
        "Your decision is queued (position {}). The user has another decision currently active — re-surface \
         that ACTIVE decision's card via `xmtp_prompt_user`, with a short notice on top that a new decision \
         is queued. The user answers the ACTIVE card; their reply routes (via `pending-decisions-v2 resolve`) \
         to the active entry's sub. Your queued card will be auto-rendered later when the active resolves.\n\n\
         🌐 **LOCALIZE FIRST**: the two English header lines in userContent below (the `🆕 A new decision \
         arrived...` top notice AND the `─── Current active decision ───` divider) must be translated to the \
         **same language as the embedded active decision text** before xmtp_prompt_user. Keep the embedded \
         active card text intact (do NOT re-translate). Do NOT send mixed-language userContent.\n\n\
         Call xmtp_prompt_user with the EXACT arguments below (after applying the header translation). End \
         the turn after the tool returns 'sent'. Do NOT call any other tool first or after.\n\n\
         tool: xmtp_prompt_user\n\
         llmContent:\n{}\n\
         userContent:\n{}\n",
        queued_position,
        indent(&llm_content, "  "),
        indent(&user_content_wrapped, "  "),
    )
}

fn playbook_relay_only(sub_key: &str, relay_content: &str) -> String {
    format!(
        "Relay the user's decision to the just-resolved sub session, then end the turn.\n\n\
         tool: xmtp_dispatch_session\n\
         sessionKey: {}\n\
         content: {}\n\n\
         ⚠️ Call `xmtp_dispatch_session` **exactly once** — when the tool returns 'Message dispatched' = end \
         the turn immediately (no more xmtp / Exec calls). Repeated calls cause sub to receive N identical \
         relays → event-recursion loop. Skipping it = sub never gets the user's decision = task stalls.\n",
        sub_key, relay_content
    )
}

fn playbook_relay_and_render(
    resolved_sub_key: &str,
    relay_content: &str,
    next: &PendingEntry,
) -> String {
    let llm_content = resolve_llm_content(next);
    // Canonical English transition prefix so the user knows "previous handled,
    // next coming". Sub LLM must translate this prefix to the user's language
    // before xmtp_prompt_user (the embedded next.user_content is already in the
    // user's language — do NOT re-translate it).
    let next_user_content = format!(
        "✓ Previous decision handled. Here's the next pending one:\n\n{}",
        next.user_content,
    );
    format!(
        "Execute the following in order WITHIN THIS TURN. End the turn after Step 2.\n\n\
         🛑 **Step 2 is `xmtp_prompt_user` (rendering), NOT another `resolve` call**. Do NOT call \
         `pending-decisions-v2 resolve` again in this turn — the next resolve only happens in a FUTURE \
         turn after the user replies to the prompt rendered in Step 2.\n\n\
         Step 1 — Relay the user's decision to the just-resolved sub session (call `xmtp_dispatch_session` \
         exactly once; repeated calls = sub receives N relays):\n\
           tool: xmtp_dispatch_session\n\
           sessionKey: {}\n\
           content: {}\n\n\
         Step 2 — Auto-render the only remaining decision.\n\
         🌐 **LOCALIZE FIRST**: the transition header `✓ Previous decision handled. Here's the next pending \
         one:` is canonical English; translate it to match the embedded next-decision's language before \
         xmtp_prompt_user. Keep the embedded next-decision text intact (do NOT re-translate). Do NOT send \
         mixed-language userContent.\n\n\
           tool: xmtp_prompt_user\n\
           llmContent:\n{}\n\
           userContent:\n{}\n",
        resolved_sub_key,
        relay_content,
        indent(&llm_content, "    "),
        indent(&next_user_content, "    "),
    )
}

fn playbook_relay_and_list(
    resolved_sub_key: &str,
    relay_content: &str,
    snap: &DisplaySnapshot,
) -> String {
    let mut list = String::new();
    list.push_str(&format!(
        "✓ Previous decision handled. {} more pending — please pick one to answer first:\n\n",
        snap.items.len()
    ));
    for it in &snap.items {
        list.push_str(&format!("{}. {}\n", it.index, it.list_label));
    }
    list.push_str(&format!(
        "\nReply with a number 1-{} to choose, or say `later` to defer.\n",
        snap.items.len()
    ));
    format!(
        "Execute the following in order WITHIN THIS TURN. End the turn after Step 2.\n\n\
         🛑 **Do NOT call `pending-decisions-v2 resolve` again in this turn** — the next CLI call is \
         `pending-decisions-v2 pick --index N` (in a FUTURE turn, after the user types a number).\n\n\
         Step 1 — Relay the user's decision to the just-resolved sub session (call `xmtp_dispatch_session` \
         exactly once):\n\
           tool: xmtp_dispatch_session\n\
           sessionKey: {}\n\
           content: {}\n\n\
         Step 2 — Render the list below VERBATIM to the user in your assistant response (text rendering only; \
         do NOT call any tool for Step 2).\n\
         🌐 **LOCALIZE FIRST**: the English header (`✓ Previous decision handled...`) and footer (`Reply with \
         a number...`) must be translated to the user's language before rendering. The list items \
         (`N. <label>`) are sub-provided and already localized; keep them intact.\n\n\
         Verbatim:\n\"\"\"\n{}\"\"\"\n\n\
         ⚠️ Next user reply routing (future turn — the queue is now in **selection mode**: 0 active + {} queued):\n\
           - Number 1-{} → `onchainos agent pending-decisions-v2 pick --index <N>` (this promotes the chosen entry to Active and renders its card)\n\
           - Defer keyword ({}) → just end the turn (the list will re-render later when the user comes back)\n\
           - Else → **DO NOT call `resolve`** — there is no active entry to resolve in selection mode. Instead, render this text to the user (translated to their language):\n\
             \"\"\"\n\
             I see your message \"<user verbatim>\" but {} decisions are still waiting; please pick one by number (1-{}) first, then I'll relay your answer to that one.\n\
             \n\
             [re-render the list above verbatim]\n\
             \"\"\"\n\
           ❌ NEVER call `resolve` while the queue has 0 active entries — it will return a stale-relist playbook (since v2.1 the CLI heals this case instead of dropping the reply, but it still costs a round-trip).\n",
        resolved_sub_key,
        relay_content,
        list,
        snap.items.len(),
        snap.items.len(),
        DEFER_KEYWORDS.join(" / "),
        snap.items.len(),
        snap.items.len(),
    )
}

fn playbook_render(entry: &PendingEntry) -> String {
    let llm_content = resolve_llm_content(entry);
    format!(
        "Render the user's selected decision. Call `xmtp_prompt_user` ONCE with the EXACT arguments below, \
         then **END THE TURN IMMEDIATELY**.\n\n\
         🛑🛑🛑 **DO NOT call `resolve` in this same turn**. The user's earlier message in this turn (e.g. \
         the digit `1`, `2`, `选 1`, etc.) was the **pick index** — NOT a decision content. Re-using it as \
         `--user-reply` would relay a nonsensical digit to the freshly-promoted sub session, corrupting the \
         decision flow. Real incident: user typed `1` to choose decision #1, master rendered the card, then \
         immediately called `resolve --user-reply \"1\"` — the digit `1` was sent as the answer to decision \
         #1, which expected something like `approve` / `reject reason: ...`.\n\n\
         The real decision content will arrive in a **FUTURE turn** as a fresh user_message after the user \
         reads the rendered card. Wait for that.\n\n\
         tool: xmtp_prompt_user\n\
         llmContent:\n{}\n\
         userContent:\n{}\n",
        indent(&llm_content, "  "),
        indent(&entry.user_content, "  "),
    )
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