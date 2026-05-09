//! Evaluator 端任务命令 — handler 模块集合
//!
//! 命令名映射（旧 → 新，CLI 调用形式）：
//! - `agent evaluator claim`           → `agent arbitration-claim`
//! - `agent evaluator claimable`       → `agent arbitration-claimable`
//!
//! - `helpers.rs`         — disputeId 解析
//! - `flow.rs`            — 状态机提示词生成器（供 `next-action --role evaluator` 使用）

pub mod claim;
pub mod claimable;
pub mod commit;
pub mod decimal_str;
pub mod download;
pub mod flow;
pub mod helpers;
pub mod increase_stake;
pub mod info;
pub mod my_stake;
pub mod reveal;
pub mod stake;
pub mod staking_config;
pub mod staking_types;
pub mod unstake;
