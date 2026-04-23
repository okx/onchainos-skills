//! Task system modules.
//!
//! 顶层 CLI 入口统一通过 `agent_commerce::AgentCommand` 暴露，
//! 本模块只提供 client / provider / evaluator / common / signing 子模块实现。

pub mod client;
pub mod common;
pub mod evaluator;
pub mod provider;
pub mod signing;
