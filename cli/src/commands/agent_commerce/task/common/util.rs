//! common::util — 任务系统通用工具函数
//!
//! 收敛 task 模块里被多处复用、与具体业务无关的小工具，避免散落在各 mod / flow 中。
//! 后续新增的展示格式化、字符串归一化、时间换算等通用 helper 都放这里。

use chrono::{TimeZone, Utc};

/// unix 秒 → 展示字符串。0 / 负数当未设置；正常值转 RFC 3339。
pub fn fmt_unix_secs(secs: Option<i64>) -> String {
    match secs {
        Some(n) if n > 0 => Utc
            .timestamp_opt(n, 0)
            .single()
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| n.to_string()),
        _ => "—".to_string(),
    }
}
