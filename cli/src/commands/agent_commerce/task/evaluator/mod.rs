//! Evaluator 端任务命令 — handler 模块集合
//!
//! 子命令已在 2026-05 重构中**展平到 `agent` 顶层**（与 buyer/provider 风格对齐），
//! 不再有 `Evaluator(EvaluatorCommand)` 包装。各 handler 由 `agent_commerce/mod.rs`
//! 直接调用；本模块只负责暴露子模块。
//!
//! 命令名映射（旧 → 新，CLI 调用形式）：
//! - `agent evaluator info`            → `agent evidence-info`
//! - `agent evaluator download`        → `agent evidence-download`
//! - `agent evaluator commit`          → `agent vote-commit`
//! - `agent evaluator reveal`          → `agent vote-reveal`
//! - `agent evaluator claim`           → `agent arbitration-claim`
//! - `agent evaluator claimable`       → `agent arbitration-claimable`
//! - `agent evaluator stake`           → `agent stake`
//! - `agent evaluator increase-stake`  → `agent increase-stake`
//! - `agent evaluator request-unstake` → `agent request-unstake`
//! - `agent evaluator claim-unstake`   → `agent claim-unstake`
//! - `agent evaluator cancel-unstake`  → `agent cancel-unstake`
//! - `agent evaluator staking-config`  → `agent staking-config`
//! - `agent evaluator my-stake`        → `agent my-stake`
//!
//! 按动作划分文件：
//! - `info.rs`            — 拉取证据（只读，含图片下载）
//! - `download.rs`        — 按 (jobId, fileKey) 单独下载一份证据字节
//! - `commit.rs`          — Commit 投票（commit-reveal 第一阶段）
//! - `reveal.rs`          — Reveal 投票（第二阶段；后端反查 vote+salt，CLI 不传 side）
//! - `claim.rs`           — account 级 pull 领取所有已结算奖励
//! - `claimable.rs`       — 查询账户待领奖励（只读）
//! - `stake.rs`           — 首次质押（身份 skill 跳转入口）
//! - `increase_stake.rs`  — 追加质押（top-up / 补齐）
//! - `unstake.rs`         — 解质押生命周期（request / claim / cancel）
//!
//! 辅助：
//! - `helpers.rs`         — disputeId 解析
//! - `flow.rs`            — 状态机提示词生成器（供 `next-action --role evaluator` 使用）

pub mod claim;
pub mod claimable;
pub mod commit;
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
