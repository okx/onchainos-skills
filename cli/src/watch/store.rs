use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use serde_json::Value;

use super::types::{DaemonState, WatchConfig};

const MAX_FILE_SIZE: u64 = 32 * 1024 * 1024; // 32 MB
const MAX_FILES: u32 = 3;

/// Root directory for all watch sessions: `~/.onchainos/watch/`
pub fn watch_root() -> Result<PathBuf> {
    Ok(crate::home::onchainos_home()?.join("watch"))
}

/// Directory for a specific watch session.
pub fn watch_dir(id: &str) -> Result<PathBuf> {
    Ok(watch_root()?.join(id))
}

fn events_path(dir: &Path, channel: &str, file_no: u32) -> PathBuf {
    dir.join(format!("events.{}.{}.jsonl", channel, file_no))
}

fn cursor_path(dir: &Path, channel: &str) -> PathBuf {
    dir.join(format!("cursor.{}", channel))
}

fn status_path(dir: &Path) -> PathBuf {
    dir.join("status")
}

fn pid_path(dir: &Path) -> PathBuf {
    dir.join("pid")
}

fn config_path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

// ── Init ─────────────────────────────────────────────────────────────────────

pub fn init_watch_dir(id: &str, config: &WatchConfig) -> Result<PathBuf> {
    let dir = watch_dir(id)?;
    fs::create_dir_all(&dir)?;
    let cfg_json = serde_json::to_string_pretty(config)?;
    // Atomic write: tmp + rename, consistent with status/cursor writes
    let tmp = dir.join(".config.json.tmp");
    fs::write(&tmp, cfg_json)?;
    fs::rename(&tmp, config_path(&dir))?;
    // Cursor is initialised lazily per channel on first write
    Ok(dir)
}

// ── PID ──────────────────────────────────────────────────────────────────────

pub fn write_pid(dir: &Path, pid: u32) -> Result<()> {
    fs::write(pid_path(dir), pid.to_string())?;
    Ok(())
}

pub fn read_pid(id: &str) -> Result<u32> {
    let dir = watch_dir(id)?;
    let s = fs::read_to_string(pid_path(&dir))?;
    Ok(s.trim().parse()?)
}

// ── Status ───────────────────────────────────────────────────────────────────

pub fn write_status(dir: &Path, state: &str, reason: Option<&str>) -> Result<()> {
    let now = now_ms();
    let line = match reason {
        Some(r) => format!("{}|{}|{}", state, now, r),
        None => format!("{}|{}", state, now),
    };
    // Write to temp then rename for atomicity
    let tmp = dir.join(".status.tmp");
    fs::write(&tmp, &line)?;
    fs::rename(&tmp, status_path(dir))?;
    Ok(())
}

pub fn read_daemon_state(id: &str) -> Result<DaemonState> {
    let dir = watch_dir(id)?;
    let status_file = status_path(&dir);
    if !status_file.exists() {
        return Ok(DaemonState::Crashed);
    }
    let line = fs::read_to_string(status_file)?;
    Ok(DaemonState::from_status_line(&line, now_ms()))
}

// ── Config ───────────────────────────────────────────────────────────────────

pub fn read_config(id: &str) -> Result<WatchConfig> {
    let dir = watch_dir(id)?;
    let s = fs::read_to_string(config_path(&dir))?;
    Ok(serde_json::from_str(&s)?)
}

// ── Cursor ───────────────────────────────────────────────────────────────────

/// Cursor: which file and byte offset poll has read up to.
#[derive(Debug, Clone, Copy)]
pub struct Cursor {
    pub file_no: u32,
    pub offset: u64,
}

pub fn read_cursor(dir: &Path, channel: &str) -> Cursor {
    let Ok(s) = fs::read_to_string(cursor_path(dir, channel)) else {
        return Cursor {
            file_no: 0,
            offset: 0,
        };
    };
    let parts: Vec<&str> = s.trim().splitn(2, '|').collect();
    if parts.len() == 2 {
        let file_no = parts[0].parse().unwrap_or(0);
        let offset = parts[1].parse().unwrap_or(0);
        Cursor { file_no, offset }
    } else {
        Cursor {
            file_no: 0,
            offset: 0,
        }
    }
}

pub fn write_cursor(dir: &Path, channel: &str, file_no: u32, offset: u64) -> Result<()> {
    let tmp = dir.join(format!(".cursor.{}.tmp", channel));
    fs::write(&tmp, format!("{}|{}", file_no, offset))?;
    fs::rename(&tmp, cursor_path(dir, channel))?;
    Ok(())
}

// ── Event append (daemon side) ────────────────────────────────────────────────

/// Append events to events.<channel>.0.jsonl, rotating if needed.
pub fn append_events(dir: &Path, channel: &str, events: &[Value]) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }

    // Check if rotation needed
    let current = events_path(dir, channel, 0);
    if current.exists() {
        let meta = fs::metadata(&current)?;
        if meta.len() >= MAX_FILE_SIZE {
            rotate_files(dir, channel)?;
        }
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(events_path(dir, channel, 0))?;

    for event in events {
        let line = serde_json::to_string(event)?;
        writeln!(file, "{}", line)?;
    }
    Ok(())
}

/// rename events.<channel>.N -> events.<channel>.N+1, drop oldest beyond MAX_FILES
fn rotate_files(dir: &Path, channel: &str) -> Result<()> {
    let oldest = events_path(dir, channel, MAX_FILES - 1);
    if oldest.exists() {
        fs::remove_file(oldest)?;
    }
    for n in (0..MAX_FILES - 1).rev() {
        let src = events_path(dir, channel, n);
        let dst = events_path(dir, channel, n + 1);
        if src.exists() {
            fs::rename(&src, dst)?;
        }
    }
    Ok(())
}

// ── Event read (poll side) ────────────────────────────────────────────────────

pub struct PollResult {
    pub events: Vec<Value>,
    /// Cursor position after each event — `per_event_cursors[i]` is the cursor
    /// right after `events[i]` was read.  Used to commit the cursor only up to
    /// the last event that survived filtering.
    pub per_event_cursors: Vec<Cursor>,
    pub new_cursor: Cursor,
}

/// Read up to `limit` complete lines from the events files for a specific channel.
pub fn read_events_from_cursor(dir: &Path, channel: &str, limit: usize) -> Result<PollResult> {
    let mut cursor = read_cursor(dir, channel);
    let mut events = Vec::new();
    let mut per_event_cursors = Vec::new();

    let path = events_path(dir, channel, cursor.file_no);

    // Detect rotation: the file at cursor.file_no either no longer exists, or its
    // current size is smaller than our saved offset. Both cases mean the file was
    // rotated (renamed to file_no+1) while we weren't looking. Drain the tail of
    // the rotated file first so those events are not lost, then fall through to
    // reading fresh data from the new file at cursor.file_no.
    let rotated = if !path.exists() {
        true
    } else {
        fs::metadata(&path)?.len() < cursor.offset
    };

    if rotated {
        let rotated_path = events_path(dir, channel, cursor.file_no + 1);
        if rotated_path.exists() {
            drain_file(&rotated_path, cursor.offset, limit, &mut events, None)?;
            // We don't persist a cursor into the rotated file;
            // the loop below will advance cursor in the new file.
        }
        // Reset cursor to the fresh file at cursor.file_no, beginning.
        cursor = Cursor {
            file_no: cursor.file_no,
            offset: 0,
        };
    }

    // Normal read from the (possibly reset) cursor position.
    let path = events_path(dir, channel, cursor.file_no);
    if path.exists() {
        let remaining = limit.saturating_sub(events.len());
        let new_offset = drain_file(
            &path,
            cursor.offset,
            remaining,
            &mut events,
            Some(&mut per_event_cursors),
        )?;
        // Fill cursor info: per_event_cursors tracks positions for events read from the current file.
        // Events from the rotated file don't get per-event cursors (they are behind the new file).
        // Pad the front with the reset cursor so indices align with `events`.
        let rotated_count = events.len() - per_event_cursors.len();
        if rotated_count > 0 {
            let pad_cursor = Cursor {
                file_no: cursor.file_no,
                offset: 0,
            };
            let mut padded = vec![pad_cursor; rotated_count];
            padded.append(&mut per_event_cursors);
            per_event_cursors = padded;
        }
        cursor.offset = new_offset;
    }

    Ok(PollResult {
        events,
        per_event_cursors,
        new_cursor: cursor,
    })
}

/// Read up to `limit` complete JSONL lines from `path` starting at byte `offset`.
/// Returns the new file offset after reading.  When `per_event_cursors` is
/// provided, each successfully parsed event records the cursor position right
/// after that line so callers can commit a partial read.
fn drain_file(
    path: &Path,
    offset: u64,
    limit: usize,
    events: &mut Vec<Value>,
    mut per_event_cursors: Option<&mut Vec<Cursor>>,
) -> Result<u64> {
    let file_no = parse_file_no_from_path(path);
    let mut file = fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut current_offset = offset;
    loop {
        if events.len() >= limit {
            break;
        }
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 || !line.ends_with('\n') {
            break; // EOF or incomplete line
        }
        current_offset += n as u64;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<Value>(trimmed) {
            events.push(event);
            if let Some(ref mut cursors) = per_event_cursors {
                cursors.push(Cursor {
                    file_no,
                    offset: current_offset,
                });
            }
        }
    }
    Ok(current_offset)
}

/// Extract the file number from an events path like `events.channel.0.jsonl`.
fn parse_file_no_from_path(path: &Path) -> u32 {
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.rsplit('.').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

// ── Watch list ────────────────────────────────────────────────────────────────

pub struct WatchEntry {
    pub id: String,
    pub state: DaemonState,
    pub pid: Option<u32>,
    pub config: Option<WatchConfig>,
}

pub fn list_watches() -> Result<Vec<WatchEntry>> {
    let root = watch_root()?;
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let id = entry.file_name().to_string_lossy().to_string();
        if !id.starts_with("ws_") && !id.starts_with("watch_") {
            continue;
        }
        let state = read_daemon_state(&id).unwrap_or(DaemonState::Crashed);
        let pid = read_pid(&id).ok();
        let config = read_config(&id).ok();
        entries.push(WatchEntry {
            id,
            state,
            pid,
            config,
        });
    }
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(entries)
}

/// Remove watch directory (called by watch stop after daemon is killed).
pub fn remove_watch_dir(id: &str) -> Result<()> {
    let dir = watch_dir(id)?;
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Return the most recent mtime (as unix ms) across all `cursor.*` files in `dir`.
/// Returns `None` if no cursor files exist (session has never been polled).
pub fn last_poll_time(dir: &Path) -> Option<u64> {
    let mut latest: Option<u64> = None;
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("cursor.") {
            if let Ok(meta) = entry.metadata() {
                if let Ok(mtime) = meta.modified() {
                    let ms = mtime.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
                    latest = Some(latest.map_or(ms, |prev: u64| prev.max(ms)));
                }
            }
        }
    }
    latest
}
