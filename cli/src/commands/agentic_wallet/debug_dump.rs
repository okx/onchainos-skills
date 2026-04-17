//! TEMPORARY: Gas Station 联调 debug dump
//! 原因: 联调阶段需要记录每次 API 调用的完整请求/响应，方便排查问题
//! 原本: 无此模块
//! 去除时机: Gas Station 联调完成、功能稳定上线后，删除此文件 + mod.rs 中的 pub mod debug_dump + transfer.rs / gas_station.rs 中所有 debug_dump::dump() 调用
//! 定位方式: 搜索 "debug_dump::dump" 找到所有调用点，搜索 "TEMPORARY: debug_dump" 找到此文件
//!
//! 功能: 将最后一次 API 请求/响应覆盖写入 ~/.onchainos/gas-station-debug/

use serde_json::Value;
use std::fs;
use std::path::PathBuf;

const DEBUG_DIR: &str = "gas-station-debug";

fn debug_dir() -> Option<PathBuf> {
    let home = crate::home::onchainos_home().ok()?;
    let dir = home.join(DEBUG_DIR);
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Dump an API request + response pair to a named JSON file.
/// Silently no-ops if the directory cannot be created.
pub fn dump(name: &str, request: &Value, response: &Value) {
    if let Some(dir) = debug_dir() {
        let payload = serde_json::json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "request": request,
            "response": response,
        });
        let path = dir.join(format!("{}.json", name));
        let _ = fs::write(&path, serde_json::to_string_pretty(&payload).unwrap_or_default());
    }
}

/// Dump request + error for failed API calls.
pub fn dump_error(name: &str, request: &Value, error: &str) {
    if let Some(dir) = debug_dir() {
        let payload = serde_json::json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "request": request,
            "response": null,
            "error": error,
        });
        let path = dir.join(format!("{}.json", name));
        let _ = fs::write(&path, serde_json::to_string_pretty(&payload).unwrap_or_default());
    }
}
