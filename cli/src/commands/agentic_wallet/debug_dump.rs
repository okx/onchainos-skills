//! TEMPORARY: Gas Station 联调 debug dump
//! 原因: 联调阶段需要记录每次 API 调用的完整请求/响应，方便排查问题
//! 原本: 无此模块
//! 去除时机: Gas Station 联调完成、功能稳定上线后，删除此文件 + mod.rs 中的 pub mod debug_dump + transfer.rs / gas_station.rs 中所有 debug_dump::dump() 调用
//! 定位方式: 搜索 "debug_dump::dump" 找到所有调用点，搜索 "TEMPORARY: debug_dump" 找到此文件
//!
//! 功能:
//! - 将最后一次 API 请求/响应写入 ~/.onchainos/gas-station-debug/<name>.json（覆盖，方便看最新）
//! - 同一份数据附带时间戳写入 ~/.onchainos/gas-station-debug/history/<iso-ts>-<name>.json（不覆盖）
//! - 每次写入前扫 history 目录，清理超过 1h 的旧文件

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const DEBUG_DIR: &str = "gas-station-debug";
const HISTORY_DIR: &str = "history";
const HISTORY_TTL_SECS: u64 = 3600; // 1 hour

fn debug_dir() -> Option<PathBuf> {
    let home = crate::home::onchainos_home().ok()?;
    let dir = home.join(DEBUG_DIR);
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn history_dir() -> Option<PathBuf> {
    let dir = debug_dir()?.join(HISTORY_DIR);
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Delete files in the given directory whose mtime is older than `HISTORY_TTL_SECS`.
/// Silently no-ops on any IO error.
pub(crate) fn cleanup_old(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let Ok(mtime) = meta.modified() else { continue };
        let Ok(age) = now.duration_since(mtime) else {
            continue;
        };
        if age.as_secs() > HISTORY_TTL_SECS {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn history_filename(name: &str) -> String {
    let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S%6fZ");
    // sanitize name for filesystem
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    format!("{}-{}.json", ts, safe)
}

/// Dump an API request + response pair.
/// Writes two places:
/// - `<debug>/{name}.json` (overwrites — convenience for "latest")
/// - `<debug>/history/<iso-ts>-{name}.json` (timestamped, preserved for 1h)
pub fn dump(name: &str, request: &Value, response: &Value) {
    let Some(dir) = debug_dir() else { return };
    let payload = serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "request": request,
        "response": response,
    });
    let rendered = serde_json::to_string_pretty(&payload).unwrap_or_default();
    // latest
    let _ = fs::write(dir.join(format!("{}.json", name)), &rendered);
    // history with cleanup
    if let Some(hist) = history_dir() {
        cleanup_old(&hist);
        let _ = fs::write(hist.join(history_filename(name)), &rendered);
    }
}

/// Dump request + error for failed API calls. Same dual-write as `dump`.
pub fn dump_error(name: &str, request: &Value, error: &str) {
    let Some(dir) = debug_dir() else { return };
    let payload = serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "request": request,
        "response": null,
        "error": error,
    });
    let rendered = serde_json::to_string_pretty(&payload).unwrap_or_default();
    let _ = fs::write(dir.join(format!("{}.json", name)), &rendered);
    if let Some(hist) = history_dir() {
        cleanup_old(&hist);
        let _ = fs::write(hist.join(history_filename(name)), &rendered);
    }
}
