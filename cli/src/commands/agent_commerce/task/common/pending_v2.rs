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
            let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法获取 HOME 目录"))?;
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
        print!("{}", playbook_error_no_active());
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
                    "ℹ️ 距上次查看,已自动清理 {} 条超过 {} 天未处理的决策。\n",
                    evicted, ttl_days,
                );
            }
            if q.entries.is_empty() {
                println!("(no pending decisions)");
            } else {
                println!("当前 {} 条待处理:\n", q.entries.len());
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
         ⚠️ **This llmContent block is for YOUR (user-session LLM's) instructions only — invisible to the user.** Render ONLY the userContent block below to the user; do **NOT** echo / paraphrase / summarize / include any part of this llmContent (including the HARDSTOP, routing rules, or this very note) in your response to the user. The user sees the userContent verbatim and nothing else.\n\n\
         Phase 1 (THIS turn): Render userContent below verbatim to the user. End the turn. Do NOT call any tool.\n\n\
         🛑🛑🛑 **HARDSTOP — what counts as `user replied`**: the ONLY valid user reply is an **actual user message arriving as an inbound in a LATER turn's tool_result** (NOT this turn). After rendering userContent, you MUST end the turn; the user has NOT spoken yet. Your own thinking / narration / paraphrasing the userContent options does **NOT** count as the user replying. **Specifically forbidden**: synthesizing a decision from the option list; reusing prior chat messages / task title / earlier inbound as a user decision; fabricating or hallucinating user wording. If in this turn there is no genuine user-input inbound, **`resolve` is forbidden — full stop, no exceptions**. Violating this rule = on-chain action executed without the user actually choosing = **potential escrow loss**.\n\n\
         Phase 2 (NEXT turn, when the user actually replies):\n\
           - Defer keyword ({}) → just end the turn; do NOT call resolve.\n\
           - Otherwise → call `onchainos agent pending-decisions-v2 resolve --user-reply \"<verbatim user wording, no interpretation, no translation>\"` **exactly once**. After CLI returns the relay playbook, execute its Step 1 once and end the turn. Repeated `resolve` calls cause sub to receive N identical relays → event-recursion loop.",
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

/// Queued + re-prompt: a genuinely new sub_key landed in the queue. The new
/// queued sub pushes **its own user_content** (with a "🆕 queued, please answer
/// the active one first" wrapper) via `xmtp_prompt_user`. The llmContent points
/// at the ACTIVE entry's sub_key so any user reply still routes (via `resolve`)
/// to the active decision — the single-active invariant is preserved.
///
/// Why this design: the sub-C LLM has its own `--user-content` from `request`
/// and naturally wants to push that. If we tried to make sub-C re-push the
/// active card's content (which belongs to a different sub), most LLMs ignore
/// the playbook and push their own content anyway. Aligning with the LLM's
/// natural inclination + wrapping the content with queued context is the
/// compliant path. User clearly sees: "new decision arrived (preview below),
/// answer the active one first; this one will re-show once you finish that".
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
    // Canonical English wrapper. Sub LLM is responsible for translating the
    // wrapper lines (the 🆕 header + the closing divider) to the user's
    // language before xmtp_prompt_user. The embedded new_entry.user_content is
    // already in the user's language (sub-C localized at request time); do NOT
    // re-translate it.
    let user_content_wrapped = format!(
        "🆕 **A new decision just arrived (queued — position {} of {} total).** \
         Another decision is currently active and waiting for your reply — \
         **please answer the active decision first** (shown earlier in the chat); \
         this queued one will auto-display once you finish that.\n\n\
         ─────────── Preview of this queued decision ───────────\n\
         {}",
        queued_position, total_pending, new_entry.user_content,
    );
    format!(
        "Your decision is queued (position {}). Push your own card to the user with a 'queued, answer the \
         active one first' wrapper so the user knows it's pending behind the active decision. The user's \
         reply (if any) routes to the ACTIVE entry via `pending-decisions-v2 resolve`, NOT to yours; your \
         queued card will be auto-rendered later when active resolves.\n\n\
         🌐🌐🌐 **MANDATORY LOCALIZATION — translate the wrapper BEFORE calling xmtp_prompt_user**:\n\
         The userContent block below is in **canonical English**, but the embedded card text (between the \
         `─── Preview ───` dividers) is **already in the user's language** (the sub localized it when calling \
         `request`). Inspect the embedded card's language and **translate the English wrapper lines to match** \
         — i.e. the `🆕 A new decision just arrived (queued — position X of Y total). Another decision is \
         currently active and waiting for your reply — please answer the active decision first (shown earlier \
         in the chat); this queued one will auto-display once you finish that.` opener AND the `─── Preview of \
         this queued decision ───` divider. Keep the embedded card text intact (do NOT re-translate).\n\
         Examples: if the embedded card is in Chinese, translate the wrapper to Chinese. If Japanese, \
         translate to Japanese. If Spanish, translate to Spanish. Do NOT leave the wrapper in English when \
         the embedded card is non-English — the user will see a confusing bilingual mix.\n\n\
         After localizing the wrapper, call xmtp_prompt_user with the resulting arguments. Do NOT modify the \
         llmContent. Do NOT call any other tool first or after. End the turn after the tool returns 'sent'.\n\n\
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
        "Relay user's decision, then end the turn.\n\n\
         tool: xmtp_dispatch_session\n\
         sessionKey: {}\n\
         content: {}\n\n\
         ⚠️ Call `xmtp_dispatch_session` **exactly once** — when the tool returns `Message dispatched` = success = **immediately terminate all subsequent tool calls in this turn** (no more `xmtp_dispatch_session` / `xmtp_send` / `xmtp_dispatch_user` / `Exec`). Repeated calls (even with identical sessionKey / content) cause sub to receive N identical relays → event-recursion loop. **Violating this rule = potential escrow loss**.\n",
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
         🛑🛑🛑 **Step 2 is `xmtp_prompt_user` (rendering), NOT another `resolve` call**. Do NOT call \
         `onchainos agent pending-decisions-v2 resolve` again in this turn. The next `resolve` call should \
         only happen in a FUTURE turn, after the user actually replies to the prompt rendered in Step 2. \
         Calling resolve twice in one turn = the newly-promoted active gets drained instantly = potential \
         escrow loss (relay dispatched for a decision the user never saw).\n\n\
         Step 1 — Relay user's decision to the just-resolved sub session (call `xmtp_dispatch_session` **exactly once**; repeated calls = sub receives N relays = event-recursion loop = potential escrow loss):\n\
           tool: xmtp_dispatch_session\n\
           sessionKey: {}\n\
           content: {}\n\n\
         Step 2 — Auto-render the only remaining decision (this is `xmtp_prompt_user` ONLY — do NOT call resolve here).\n\
         🌐🌐🌐 **MANDATORY LOCALIZATION** for Step 2: the transition header `✓ Previous decision handled. Here's the \
         next pending one:` is **canonical English**. The embedded next-decision text is **already in the user's \
         language**. Inspect the embedded text and **translate the transition header to match** before xmtp_prompt_user. \
         If embedded is Chinese → translate header to Chinese (e.g. `✓ 上一项已处理完。下面是下一条待决策:`); if \
         Japanese → Japanese; etc. Do NOT leave the header in English when the embedded text is non-English. Keep \
         the embedded text intact (do NOT re-translate).\n\n\
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
         🛑🛑🛑 **Do NOT call `pending-decisions-v2 resolve` again in this turn** — the next CLI call should be \
         `pending-decisions-v2 pick --index N` (in a FUTURE turn, after the user types a number to select from \
         the list rendered in Step 2). Multiple resolves in one turn = queue drained = potential escrow loss.\n\n\
         Step 1 — Relay user's decision to the just-resolved sub session (call `xmtp_dispatch_session` **exactly once**; repeated calls = sub receives N relays = event-recursion loop = potential escrow loss):\n\
           tool: xmtp_dispatch_session\n\
           sessionKey: {}\n\
           content: {}\n\n\
         Step 2 — In your assistant response, render the list below VERBATIM (this is text rendering ONLY, NOT a resolve call).\n\
                  Do NOT add commentary / change order / change format. Do NOT call any tool.\n\
         🌐 LOCALIZE FIRST: the list header (`✓ Previous decision handled...`) and footer (`Reply with a number...`) \
         are canonical English — translate them to the user's language. The list items (`N. <label>`) are sub-provided \
         and already localized; do NOT re-translate them.\n\n\
         Verbatim:\n\"\"\"\n{}\"\"\"\n\n\
         After rendering, end the turn.\n\n\
         ⚠️ Next user reply routing (future turn):\n\
           - Number 1-{} → `onchainos agent pending-decisions-v2 pick --index <N>`\n\
           - Defer keyword ({}) → just end the turn\n\
           - Else → `onchainos agent pending-decisions-v2 resolve --user-reply \"<verbatim>\"`\n",
        resolved_sub_key,
        relay_content,
        list,
        snap.items.len(),
        DEFER_KEYWORDS.join(" / "),
    )
}

fn playbook_render(entry: &PendingEntry) -> String {
    let llm_content = resolve_llm_content(entry);
    format!(
        "Render the user's selected decision:\n\n\
         tool: xmtp_prompt_user\n\
         llmContent:\n{}\n\
         userContent:\n{}\n",
        indent(&llm_content, "  "),
        indent(&entry.user_content, "  "),
    )
}

fn playbook_error_no_active() -> String {
    "There is no active pending decision to resolve. The user's reply is likely a normal chat message.\n\
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
        list.push_str("队列已空,无需选择。\n");
    } else {
        list.push_str(&format!(
            "您刚才的选择已失效({})。当前列表:\n\n",
            reason
        ));
        for it in &snap.items {
            list.push_str(&format!("{}. {}\n", it.index, it.list_label));
        }
        list.push_str(&format!("\n回复数字 1-{} 重新选择。\n", snap.items.len()));
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