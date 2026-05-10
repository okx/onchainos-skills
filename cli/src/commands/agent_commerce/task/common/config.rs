//! Task module 全局开关——集中控制跨流程的行为差异。
//!
//! 改动时**只改这里**,各 flow.rs / content.rs 通过引用本模块常量动态出 prompt;
//! 改完需要 `cargo build` 重新生成二进制。
//!
//! 加新开关的步骤:
//! 1. 在本文件加 `pub const FOO: bool = ...;` (附 doc 注释说明 true / false 各意味着什么)
//! 2. 在用到的地方读 `super::super::common::config::FOO`(或 `crate::commands::agent_commerce::task::common::config::FOO`)
//! 3. 通常配 `if config::FOO { hint_keep } else { hint_delete }` 这种二选一字符串

/// 任务终态(`completed` / `refunded` / `close` / `dispute_resolved`)是否保留 sub session 历史。
///
/// - `true`(默认)= **保留** —— 各终态 arm 输出 "**不要 `xmtp_delete_conversation`**——保留会话历史便于事后查阅" 指令。
///   适用场景:agent 调试 / 客户支持 / 产品早期需要回查 task 全程消息。
/// - `false` = **释放** —— 各终态 arm 输出 "任务终态,可调 `xmtp_delete_conversation` 释放会话资源" 指令。
///   适用场景:大规模生产,会话过多导致前端 / IM 桥负担,需要主动清理。
pub const KEEP_CONVERSATION_ON_TERMINAL: bool = true;
