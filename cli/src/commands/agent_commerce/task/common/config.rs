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

/// 任务协议版本号 —— 双向同一个数,既是本地"我目前在哪个版本",也是
/// "我要求 peer 至少在哪个版本"。
///
/// - **发送方**:每次 `xmtp_send` 把本值塞进 `payload.taskMinVersion`
/// - **接收方**:next-action 用 `--peerTaskMinVersion` 拿到 peer 的值,
///   判定 `本地 TASK_MIN_VERSION < peer.taskMinVersion` ⇒ 本地过期,
///   输出 version_mismatch 剧本,提示用户 `onchainos upgrade`
///
/// Bump 规则:仅当任务协议(状态机 / envelope schema / payload schema)发生
/// **破坏向后兼容**的变化时 +1;纯 bug fix / 文案微调不要动。
pub const TASK_MIN_VERSION: u32 = 1;
